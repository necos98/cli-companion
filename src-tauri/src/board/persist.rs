//! Formato su disco della board: la busta `{schema_version, data}` del modulo
//! `store`, la catena di migrazioni, le invarianti sui valori dopo la lettura e
//! la riga dell'archivio dei task conclusi. Nessun I/O: lo fa chi possiede la
//! cartella (`automation`), che crea un `JsonFile` per workspace con `board_store`.

use super::{legacy, legacy_goals, legacy_settings, Board, Task, MAX_PARALLEL_LIMIT};
use crate::agents::registry;
use crate::store::{Decoded, JsonFile, Migration};
use serde::Serialize;
use serde_json::Value;

/// Passi di migrazione della board: la versione corrente e' il loro numero.
/// Il passo 0 -> 1 porta alla busta il file storico (v0, anche della v0.11); il
/// passo 1 -> 2 sposta nelle proposte di impostazioni i campi dei worker che
/// ora sono per agente; il passo 2 -> 3 aggiunge i goal e il 3 -> 4 li toglie di nuovo.
static BOARD_MIGRATIONS: [Migration; 4] =
    [legacy::v0_to_v1, legacy_settings::v1_to_v2, legacy_goals::v2_to_v3, legacy_goals::v3_to_v4];

/// Versione di una riga dell'archivio dei task conclusi.
const ARCHIVE_VERSION: u32 = 1;

/// Il formato della board senza un file: serve a leggere un testo
/// (`Board::from_json`). Per un file vero si usa `board_store`.
static BOARD_FORMAT: JsonFile<Board> = configured(JsonFile::new("board.json", &BOARD_MIGRATIONS));

const fn configured(file: JsonFile<Board>) -> JsonFile<Board> {
    file.validated(check_board).decoded_with(decode_board)
}

/// Il file `file_name` (`<workspace>.json`) di una board, con la propria
/// protezione contro la riscrittura di un file che non si e' potuto leggere.
pub fn board_store(file_name: String) -> JsonFile<Board> {
    configured(JsonFile::named(file_name, &BOARD_MIGRATIONS))
}

fn decode_board(data: Value) -> Result<Decoded<Board>, String> {
    let mut value: Board = serde_json::from_value(data).map_err(|e| format!("dati non validi: {e}"))?;
    value.sanitize();
    Ok(Decoded { value, dropped: Vec::new() })
}

fn check_board(board: &Board) -> Result<(), String> {
    if board.workspace_id.is_empty() {
        return Err("workspace_id missing".to_string());
    }
    Ok(())
}

impl Board {
    /// Invarianti sui valori, applicate a ogni lettura: parallelismo nei limiti
    /// e un agente per ogni task.
    pub(super) fn sanitize(&mut self) {
        self.max_parallel = self.max_parallel.clamp(1, MAX_PARALLEL_LIMIT);
        for t in self.tasks.iter_mut().filter(|t| t.agent.is_empty()) {
            t.agent = registry::default_agent().id().to_string();
        }
    }

    /// Legge una board dal testo salvato (busta o file storico), con le
    /// migrazioni e il recupero dopo il riavvio: i task `running` non hanno piu'
    /// un worker e falliscono come interrotti.
    pub fn from_json(raw: &str, now: i64) -> Result<Board, String> {
        let mut board = BOARD_FORMAT.parse(raw)?;
        board.recover_after_restart(now);
        Ok(board)
    }
}

#[derive(Serialize)]
struct ArchiveLine<'a> {
    schema_version: u32,
    data: &'a Task,
}

/// Riga dell'archivio (JSON su una riga) di un task concluso e rimosso dalla board.
pub fn archive_line(task: &Task) -> Result<String, String> {
    serde_json::to_string(&ArchiveLine { schema_version: ARCHIVE_VERSION, data: task }).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
