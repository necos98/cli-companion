//! Skill globali create dall'utente: archivio e sincronizzazione. Solo logica
//! pura su path passati come parametri (nessun `AppHandle`), cosi' i test usano
//! una cartella temporanea e il sync puo' puntare a qualunque cartella delle
//! skill (oggi `~/.claude/skills` di Windows, poi quella delle distro WSL).
//!
//! Le skill NON stanno nel plugin `cli-companion-notify`: salvarne una non
//! cambia la versione del plugin ne' innesca reinstallazioni. L'archivio
//! sorgente e' `<app data>/user-skills/<nome>/` (SKILL.md generato + file
//! accessori); `sync_one` lo copia in `<skills dir>/<nome>/` aggiungendo il
//! marcatore `.cli-companion-managed`. Una cartella target senza marcatore e'
//! una skill scritta a mano dall'utente: non viene mai sovrascritta ne'
//! cancellata. Gli errori sono in inglese perche' li legge il modello via RPC.

mod validate;

#[cfg(test)]
mod tests;

pub use validate::validate;

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use validate::check_slug;

/// Nome del file di istruzioni di una skill.
pub const SKILL_FILE: &str = "SKILL.md";

/// Marcatore delle cartelle target create e gestite da cli-companion.
pub const MANAGED_MARKER: &str = ".cli-companion-managed";

const MARKER_CONTENT: &str = "Managed by cli-companion. Changes made here are overwritten.\n";

/// Cartella dell'archivio dentro la cartella dati dell'app.
const STORE_DIR_NAME: &str = "user-skills";

/// Suffisso della cartella di staging accanto alla destinazione (`.<nome>.<suffisso>`).
const STAGING_SUFFIX: &str = "cli-companion-tmp";

/// Prefisso della riga `description:` nel frontmatter generato.
const DESCRIPTION_KEY: &str = "description: ";

/// File accessorio di una skill: path relativo con `/` e contenuto testuale.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillFile {
    pub path: String,
    pub content: String,
}

/// Campi separati da cui il modulo genera il SKILL.md.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillInput {
    pub name: String,
    pub description: String,
    pub body: String,
    #[serde(default)]
    pub files: Vec<SkillFile>,
}

/// Esito di `save`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SavedSkill {
    pub name: String,
    pub description: String,
}

/// Riga dell'elenco delle skill salvate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
}

/// Esito del sync di una singola skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub name: String,
    pub result: Result<(), String>,
}

/// Archivio sorgente delle skill: `<app data>/user-skills`.
pub fn store_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(STORE_DIR_NAME)
}

/// Cartella delle skill personali di Claude Code su Windows
/// (`CLAUDE_CONFIG_DIR` o `~/.claude`, come il resto dell'app).
pub fn windows_skills_dir() -> Result<PathBuf, String> {
    crate::agents::claude_usage::windows_claude_dir().map(|dir| dir.join("skills"))
}

/// Valida e salva (o aggiorna) la skill nell'archivio. I file accessori che
/// non ci sono piu' vengono rimossi.
pub fn save(store_root: &Path, input: &SkillInput) -> Result<SavedSkill, String> {
    validate(input)?;
    let description = input.description.trim().to_string();
    publish_dir(&store_root.join(&input.name), |dir| {
        write_text(&dir.join(SKILL_FILE), &render_skill_md(input, &description))?;
        input.files.iter().try_for_each(|file| write_text(&dir.join(&file.path), &file.content))
    })?;
    Ok(SavedSkill { name: input.name.clone(), description })
}

/// Elenco delle skill salvate, ordinato per nome. Le cartelle che non sono
/// skill valide vengono ignorate.
pub fn list(store_root: &Path) -> Result<Vec<SkillInfo>, String> {
    let entries = match fs::read_dir(store_root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_context(e, "Cannot read the skills store")),
    };
    let mut skills: Vec<SkillInfo> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let text = fs::read_to_string(entry.path().join(SKILL_FILE)).ok()?;
            Some(SkillInfo { description: parse_description(&text)?, name })
        })
        .filter(|skill| validate::is_slug(&skill.name))
        .collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

/// Cancella la skill dall'archivio (non dal target: vedi `remove_from`).
pub fn delete(store_root: &Path, name: &str) -> Result<(), String> {
    check_slug(name)?;
    let dir = store_root.join(name);
    if !dir.join(SKILL_FILE).is_file() {
        return Err(format!("Skill '{name}' not found."));
    }
    fs::remove_dir_all(&dir).map_err(|e| io_context(e, &format!("Cannot delete skill '{name}'")))
}

/// Copia la skill dell'archivio in `<target_skills_dir>/<name>/`, sostituendo
/// una versione gia' gestita. Rifiuta una cartella target senza marcatore.
pub fn sync_one(store_root: &Path, name: &str, target_skills_dir: &Path) -> Result<(), String> {
    check_slug(name)?;
    let source = store_root.join(name);
    if !source.join(SKILL_FILE).is_file() {
        return Err(format!("Skill '{name}' not found."));
    }
    let dest = target_skills_dir.join(name);
    ensure_replaceable(&dest)?;
    publish_dir(&dest, |dir| {
        write_text(&dir.join(MANAGED_MARKER), MARKER_CONTENT)?;
        copy_files(&source, dir)
    })
}

/// Errore se `<target_skills_dir>/<name>` esiste e non e' gestita da
/// cli-companion: serve a rifiutare un salvataggio prima di scrivere.
pub fn check_target_free(target_skills_dir: &Path, name: &str) -> Result<(), String> {
    check_slug(name)?;
    ensure_replaceable(&target_skills_dir.join(name)).map(drop)
}

/// Rimuove la skill dal target. `Ok(false)` se non c'era; errore se la
/// cartella esiste ma non e' gestita da cli-companion.
pub fn remove_from(target_skills_dir: &Path, name: &str) -> Result<bool, String> {
    check_slug(name)?;
    let dest = target_skills_dir.join(name);
    if !ensure_replaceable(&dest)? {
        return Ok(false);
    }
    fs::remove_dir_all(&dest).map_err(|e| io_context(e, &format!("Cannot remove skill '{name}'")))?;
    Ok(true)
}

/// Sincronizza tutte le skill dell'archivio nel target (ricostruisce le
/// mancanti). Un errore su una skill non ferma le altre.
pub fn sync_all(store_root: &Path, target_skills_dir: &Path) -> Result<Vec<SyncReport>, String> {
    Ok(list(store_root)?
        .into_iter()
        .map(|skill| SyncReport {
            result: sync_one(store_root, &skill.name, target_skills_dir),
            name: skill.name,
        })
        .collect())
}

/// SKILL.md con frontmatter coerente con lo slug, fine riga LF. La description
/// e' una stringa tra doppi apici (JSON e' YAML valido): nessun carattere
/// speciale puo' rompere il frontmatter.
fn render_skill_md(input: &SkillInput, description: &str) -> String {
    let quoted = serde_json::to_string(description).expect("una stringa e' sempre serializzabile");
    let body = input.body.replace("\r\n", "\n");
    format!("---\nname: {}\n{DESCRIPTION_KEY}{quoted}\n---\n\n{}\n", input.name, body.trim())
}

/// Description dal frontmatter generato da `render_skill_md`.
fn parse_description(skill_md: &str) -> Option<String> {
    let frontmatter = skill_md.strip_prefix("---\n")?.split("\n---\n").next()?;
    let line = frontmatter.lines().find_map(|l| l.strip_prefix(DESCRIPTION_KEY))?;
    serde_json::from_str(line).ok()
}

/// `Ok(true)` se `dest` esiste ed e' una cartella gestita (sostituibile),
/// `Ok(false)` se non esiste, errore se esiste ma non e' nostra (anche un
/// symlink: non lo seguiamo).
fn ensure_replaceable(dest: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(dest) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io_context(e, &format!("Cannot inspect '{}'", dest.display()))),
        Ok(meta) if meta.is_dir() && dest.join(MANAGED_MARKER).is_file() => Ok(true),
        Ok(_) => Err(format!(
            "Refusing to touch '{}': it was not created by cli-companion (missing {MANAGED_MARKER} marker). Rename or remove it first.",
            dest.display()
        )),
    }
}

/// Costruisce la cartella in uno staging accanto a `dest` e poi la sostituisce
/// in un colpo solo: un errore a meta' lascia intatta la versione precedente.
fn publish_dir(dest: &Path, fill: impl FnOnce(&Path) -> Result<(), String>) -> Result<(), String> {
    let parent = dest.parent().ok_or_else(|| "Invalid destination.".to_string())?;
    let name = dest.file_name().and_then(|n| n.to_str()).ok_or_else(|| "Invalid destination.".to_string())?;
    let staging = parent.join(format!(".{name}.{STAGING_SUFFIX}"));
    fs::create_dir_all(parent).map_err(|e| io_context(e, "Cannot create the skills folder"))?;
    remove_dir_if_exists(&staging)?;
    fs::create_dir(&staging).map_err(|e| io_context(e, "Cannot create the staging folder"))?;
    let result = fill(&staging).and_then(|()| {
        remove_dir_if_exists(dest)?;
        fs::rename(&staging, dest).map_err(|e| io_context(e, &format!("Cannot write skill '{name}'")))
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn remove_dir_if_exists(dir: &Path) -> Result<(), String> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Err(io_context(e, &format!("Cannot remove '{}'", dir.display())))
        }
        _ => Ok(()),
    }
}

/// Scrive un file creando le cartelle intermedie.
fn write_text(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| io_context(e, "Cannot create a skill folder"))?;
    }
    fs::write(path, content).map_err(|e| io_context(e, &format!("Cannot write '{}'", path.display())))
}

/// Copia ricorsiva dei soli file regolari (i symlink non si seguono).
fn copy_files(src: &Path, dst: &Path) -> Result<(), String> {
    let entries = fs::read_dir(src).map_err(|e| io_context(e, "Cannot read the skill folder"))?;
    for entry in entries {
        let entry = entry.map_err(|e| io_context(e, "Cannot read the skill folder"))?;
        let kind = entry.file_type().map_err(|e| io_context(e, "Cannot read the skill folder"))?;
        let target = dst.join(entry.file_name());
        if kind.is_dir() {
            fs::create_dir(&target).map_err(|e| io_context(e, "Cannot create a skill folder"))?;
            copy_files(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target).map_err(|e| io_context(e, "Cannot copy a skill file"))?;
        }
    }
    Ok(())
}

fn io_context(error: io::Error, context: &str) -> String {
    format!("{context}: {error}")
}
