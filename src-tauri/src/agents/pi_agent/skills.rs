//! Contratto `UserSkills` di Pi: le skill personali stanno in
//! `<agent dir>/skills/<nome>/SKILL.md` (`$PI_CODING_AGENT_DIR` o `~/.pi/agent`).

use super::agent_dir::agent_dir;
use super::PiAgent;
use crate::agents::UserSkills;
use crate::platform::Target;
use std::path::PathBuf;

/// Cartella delle skill dentro la cartella dell'agente.
const SKILLS_DIR: &str = "skills";

impl UserSkills for PiAgent {
    fn windows_dir(&self) -> Result<PathBuf, String> {
        Ok(agent_dir(&Target::Windows)?.join(SKILLS_DIR))
    }

    /// Nella home Linux della distro (dove `PI_CODING_AGENT_DIR` non si legge).
    fn wsl_home_subpath(&self) -> &'static str {
        ".pi/agent/skills"
    }

    /// Pi carica la skill per nome: `/skill:<nome>`.
    fn skill_name(&self, name: &str) -> String {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_skill_is_loaded_by_its_name() {
        assert_eq!(PiAgent.skill_name("my-skill"), "my-skill");
    }

    #[test]
    fn distro_skills_live_in_the_pi_agent_folder_of_the_home() {
        assert_eq!(PiAgent.wsl_home_subpath(), ".pi/agent/skills");
    }
}
