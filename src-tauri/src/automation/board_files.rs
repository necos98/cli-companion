//! Le board su disco (`<app data>/boards/<workspace>.json`) e il loro archivio
//! (`boards/archive/<workspace>.jsonl`, una riga per task concluso e tolto dalla
//! board).
//! Ogni workspace ha un `JsonFile` che vive per tutta l'esecuzione: la
//! protezione contro la riscrittura di un file che non si e' potuto leggere
//! (quarantena fallita, permessi) deve valere anche per i salvataggi successivi.

use super::{lock, worktrees};
use crate::board::{archive_line, board_store, Board, Task};
use crate::store::JsonFile;
use std::collections::HashMap;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const ARCHIVE_DIR: &str = "archive";
const ARCHIVE_EXT: &str = "jsonl";

type Files = HashMap<String, Arc<JsonFile<Board>>>;

static FILES: OnceLock<Mutex<Files>> = OnceLock::new();

/// Il file della board di `ws_id`; l'id deve essere valido (non esce dalla cartella).
fn file_of(dir: &Path, ws_id: &str) -> Result<Arc<JsonFile<Board>>, String> {
    worktrees::board_file(dir, ws_id)?;
    let mut files = lock(FILES.get_or_init(Mutex::default));
    Ok(files.entry(ws_id.to_string()).or_insert_with(|| Arc::new(board_store(format!("{ws_id}.json")))).clone())
}

/// Salva la board nella busta della versione corrente. Rifiutato (con un
/// errore) se il file esistente non si e' potuto leggere ne' mettere da parte.
pub(super) fn save(dir: &Path, board: &Board) -> Result<(), String> {
    file_of(dir, &board.workspace_id)?.save(dir, board)
}

/// Legge tutte le board della cartella, pronte da caricare: migrate e con i
/// task `running` falliti come interrotti. I file inutilizzabili sono gia'
/// stati messi da parte (o lasciati dov'erano) dallo store, che ne avvisa.
pub(super) fn load_all(dir: &Path, now: i64) -> Vec<Board> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            log::error!("board: impossibile leggere la cartella {}: {e}", dir.display());
            return Vec::new();
        }
    };
    let mut boards = Vec::new();
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(e) => {
                log::error!("board: voce illeggibile in {}: {e}", dir.display());
                continue;
            }
        };
        let Some(ws_id) = workspace_of(&path) else {
            continue;
        };
        let file = match file_of(dir, ws_id) {
            Ok(file) => file,
            Err(e) => {
                log::warn!("board: file {} ignorato: {e}", path.display());
                continue;
            }
        };
        let mut board = file.load(dir).value;
        if board.workspace_id.is_empty() {
            // Inutilizzabile: l'avviso e' gia' stato emesso dallo store.
            continue;
        }
        let recovered = board.recover_after_restart(now);
        if recovered > 0 {
            log::info!("board {}: {recovered} task in corso interrotti dal riavvio", board.workspace_id);
        }
        boards.push(board);
    }
    boards
}

/// Id del workspace di un file `<id>.json` (backup e cartelle non lo sono).
fn workspace_of(path: &Path) -> Option<&str> {
    if path.extension()?.to_str()? != "json" {
        return None;
    }
    path.file_stem()?.to_str()
}

/// Cancella il file della board e i suoi archivi; il file sparito non ha piu'
/// un blocco delle scritture da ricordare. Gli errori sono nel log.
pub(super) fn delete(dir: &Path, ws_id: &str) {
    let file = match worktrees::board_file(dir, ws_id) {
        Ok(file) => file,
        Err(e) => {
            log::error!("board {ws_id}: non eliminata: {e}");
            return;
        }
    };
    lock(FILES.get_or_init(Mutex::default)).remove(ws_id);
    for path in [file, archive_file(dir, ws_id)] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => log::error!("board {ws_id}: impossibile eliminare {}: {e}", path.display()),
        }
    }
}

fn archive_file(dir: &Path, ws_id: &str) -> PathBuf {
    dir.join(ARCHIVE_DIR).join(format!("{ws_id}.{ARCHIVE_EXT}"))
}

/// Aggiunge i task all'archivio del workspace, una riga ciascuno, con una sola
/// `write` in coda al file.
pub(super) fn append_archive(dir: &Path, ws_id: &str, tasks: &[Task]) -> Result<(), String> {
    worktrees::board_file(dir, ws_id)?;
    let lines = tasks.iter().map(archive_line).collect::<Result<Vec<_>, _>>()?;
    append_lines(dir, &archive_file(dir, ws_id), &lines)
}

/// Scrive le righe in coda al file dell'archivio, con una sola `write`.
fn append_lines(dir: &Path, path: &Path, lines: &[String]) -> Result<(), String> {
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    let fail = |e: std::io::Error| format!("archivio {}: {e}", path.display());
    std::fs::create_dir_all(dir.join(ARCHIVE_DIR)).map_err(fail)?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(fail)?;
    file.write_all(text.as_bytes()).map_err(fail)
}

#[cfg(test)]
mod tests;
