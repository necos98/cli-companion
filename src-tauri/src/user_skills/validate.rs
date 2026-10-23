//! Validazione degli input di una skill utente. Gli errori li legge il modello
//! via RPC, quindi sono in inglese.

use super::{SkillFile, SkillInput, MANAGED_MARKER, SKILL_FILE};
use crate::agents::presets::PRESETS;

pub const MAX_NAME_LEN: usize = 40;
pub const MAX_DESCRIPTION_CHARS: usize = 1024;
pub const MAX_BODY_BYTES: usize = 100 * 1024;
pub const MAX_FILES: usize = 20;
pub const MAX_FILE_BYTES: usize = 200 * 1024;

/// Nomi che non sono preset ma sono riservati (il wizard di creazione skill).
const RESERVED_NAMES: &[&str] = &["skill-wizard"];

/// Separatore del frontmatter YAML: vietato nella description.
const FRONTMATTER_FENCE: &str = "---";

/// Validazione completa di una skill prima di salvarla.
pub fn validate(input: &SkillInput) -> Result<(), String> {
    validate_name(&input.name)?;
    validate_description(&input.description)?;
    validate_body(&input.body)?;
    validate_files(&input.files)
}

/// `true` se `name` e' uno slug `^[a-z0-9][a-z0-9-]{0,39}$`.
pub fn is_slug(name: &str) -> bool {
    let mut chars = name.chars();
    let ok_char = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    chars.next().is_some_and(ok_char)
        && name.len() <= MAX_NAME_LEN
        && name.chars().all(|c| ok_char(c) || c == '-')
}

/// Slug richiesto per ogni operazione che costruisce un path dal nome.
pub fn check_slug(name: &str) -> Result<(), String> {
    if is_slug(name) {
        return Ok(());
    }
    Err(format!(
        "Invalid skill name '{name}': use lowercase letters, digits and hyphens, starting with a letter or digit, max {MAX_NAME_LEN} characters."
    ))
}

/// Slug valido e non riservato (preset incorporati, wizard).
pub fn validate_name(name: &str) -> Result<(), String> {
    check_slug(name)?;
    if RESERVED_NAMES.contains(&name) || PRESETS.iter().any(|p| p.id == name) {
        return Err(format!("The skill name '{name}' is reserved."));
    }
    Ok(())
}

pub fn validate_description(description: &str) -> Result<(), String> {
    let description = description.trim();
    if description.is_empty() {
        return Err("The skill description must not be empty.".to_string());
    }
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(format!("The skill description is longer than {MAX_DESCRIPTION_CHARS} characters."));
    }
    // Ogni tipo di a-capo (anche Unicode) romperebbe il frontmatter.
    if description.chars().any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')) {
        return Err("The skill description must be a single line without control characters.".to_string());
    }
    if description.contains(FRONTMATTER_FENCE) {
        return Err(format!("The skill description must not contain '{FRONTMATTER_FENCE}'."));
    }
    Ok(())
}

pub fn validate_body(body: &str) -> Result<(), String> {
    if body.trim().is_empty() {
        return Err("The skill body must not be empty.".to_string());
    }
    if body.len() > MAX_BODY_BYTES {
        return Err(format!("The skill body is larger than {} KB.", MAX_BODY_BYTES / 1024));
    }
    if body.contains('\0') {
        return Err("The skill body must be plain text.".to_string());
    }
    Ok(())
}

pub fn validate_files(files: &[SkillFile]) -> Result<(), String> {
    if files.len() > MAX_FILES {
        return Err(format!("A skill can have at most {MAX_FILES} extra files."));
    }
    let mut seen: Vec<String> = Vec::with_capacity(files.len());
    for file in files {
        validate_file_path(&file.path)?;
        if file.content.len() > MAX_FILE_BYTES {
            return Err(format!("The file '{}' is larger than {} KB.", file.path, MAX_FILE_BYTES / 1024));
        }
        if file.content.contains('\0') {
            return Err(format!("The file '{}' must be plain text.", file.path));
        }
        // Windows e' case-insensitive: "A.md" e "a.md" sono lo stesso file.
        let key = file.path.to_lowercase();
        if seen.contains(&key) {
            return Err(format!("Duplicate file path '{}'.", file.path));
        }
        seen.push(key);
    }
    Ok(())
}

/// Path relativo con `/`, senza `..`, assoluti, drive, backslash o nomi
/// non portabili su Windows.
fn validate_file_path(path: &str) -> Result<(), String> {
    let bad = |why: &str| Err(format!("Invalid file path '{path}': {why}."));
    if path.is_empty() {
        return bad("empty");
    }
    if path.contains('\\') || path.contains(':') || path.starts_with('/') {
        return bad("use a relative path with '/' separators");
    }
    if path.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '|' | '?' | '*')) {
        return bad("it contains forbidden characters");
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return bad("empty, '.' and '..' segments are not allowed");
        }
        if segment.ends_with('.') || segment.ends_with(' ') {
            return bad("segments must not end with a dot or a space");
        }
    }
    let first = path.split('/').next().unwrap_or_default();
    if path.eq_ignore_ascii_case(SKILL_FILE) || first.eq_ignore_ascii_case(MANAGED_MARKER) {
        return bad("the name is reserved");
    }
    Ok(())
}
