//! Contratto `PluginSkills` di Pi: ogni skill sta nell'estensione (la scopre
//! l'evento `resources_discover`) e Pi la carica per nome.

use super::PiAgent;
use crate::agents::presets::Preset;
use crate::agents::PluginSkills;

/// Path della skill nel pacchetto (usato anche da `extension.rs`).
pub(super) fn skill_path(skill: &Preset) -> String {
    format!("skills/{}/SKILL.md", skill.id)
}

impl PluginSkills for PiAgent {
    fn skill_path(&self, skill: &Preset) -> String {
        skill_path(skill)
    }

    /// Pi chiama la skill con il nome della sua cartella, cioe' l'id.
    fn skill_name(&self, id: &str) -> String {
        id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::presets::PRESETS;

    #[test]
    fn skill_path_is_inside_the_extension_folder() {
        assert_eq!(PiAgent.skill_path(&PRESETS[0]), "skills/task-intake/SKILL.md");
        assert_eq!(skill_path(&PRESETS[1]), "skills/skill-wizard/SKILL.md");
    }

    #[test]
    fn the_skill_is_named_by_its_id() {
        for skill in PRESETS {
            assert_eq!(PiAgent.skill_name(skill.id), skill.id);
        }
    }
}
