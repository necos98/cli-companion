//! Preset di agenti: sessioni interattive guidate da una skill (es. Task Intake).
//!
//! Aggiungere un preset = una voce in `PRESETS` + un file in `presets/<id>.md`
//! (SKILL.md completo di frontmatter `name`/`description`, in inglese: lo legge il
//! modello). Il plugin dell'agente ne installa la skill e l'app espone una voce
//! rapida `builtin:preset:<id>` per ogni preset (vedi `collect_builtin_launches`).

/// Un preset: id (= nome della skill e della cartella), nome mostrato e SKILL.md.
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub skill: &'static str,
}

impl Preset {
    /// SKILL.md con fine riga LF, qualunque sia il checkout.
    pub fn skill_text(&self) -> String {
        self.skill.replace("\r\n", "\n")
    }

    /// Id della voce rapida integrata.
    pub fn launch_id(&self) -> String {
        format!("{PRESET_LAUNCH_PREFIX}{}", self.id)
    }
}

/// Prefisso degli id delle voci rapide dei preset.
pub const PRESET_LAUNCH_PREFIX: &str = "builtin:preset:";

/// Vecchia voce rapida dell'orchestratore (v0.11), sostituita dal preset Task Intake.
pub const LEGACY_ORCHESTRATOR_LAUNCH: &str = "builtin:orchestrator";

/// Registro dei preset.
pub static PRESETS: &[Preset] = &[Preset {
    id: "task-intake",
    name: "Task Intake",
    skill: include_str!("presets/task-intake.md"),
}];

/// Migra l'id del vecchio avvio dell'orchestratore a quello di Task Intake.
pub fn migrate_launch_id(id: &mut Option<String>) {
    if id.as_deref() == Some(LEGACY_ORCHESTRATOR_LAUNCH) {
        *id = Some(format!("{PRESET_LAUNCH_PREFIX}task-intake"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_well_formed() {
        let mut ids: Vec<_> = PRESETS.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), PRESETS.len());
        for p in PRESETS {
            assert!(p.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "{}", p.id);
            let skill = p.skill_text();
            assert!(!skill.contains('\r') && skill.is_ascii(), "{}", p.id);
            let front = skill.strip_prefix("---\n").and_then(|s| s.split_once("\n---\n")).unwrap().0;
            let field = |k: &str| front.lines().find_map(|l| l.strip_prefix(k)).map(str::trim);
            assert_eq!(field("name:"), Some(p.id));
            assert!(field("description:").is_some_and(|d| !d.is_empty() && d.len() <= 1024));
        }
    }

    #[test]
    fn task_intake_skill_covers_the_method() {
        let p = PRESETS.iter().find(|p| p.id == "task-intake").unwrap();
        assert_eq!(p.name, "Task Intake");
        assert_eq!(p.launch_id(), "builtin:preset:task-intake");
        let skill = p.skill_text();
        for t in [
            "propose_tasks", "list_tasks", "get_task_details", "update_task", "delete_task", "Goal", "Context",
            "Constraints", "Done when", "Verify", "depends_on", "Task panel",
        ] {
            assert!(skill.contains(t), "{t}");
        }
    }

    #[test]
    fn legacy_orchestrator_launch_is_migrated() {
        let mut id = Some("builtin:orchestrator".to_string());
        migrate_launch_id(&mut id);
        assert_eq!(id.as_deref(), Some("builtin:preset:task-intake"));
        for keep in [None, Some("shell".to_string()), Some("builtin:claude-code".to_string())] {
            let mut id = keep.clone();
            migrate_launch_id(&mut id);
            assert_eq!(id, keep);
        }
    }
}
