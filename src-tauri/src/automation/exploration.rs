//! La delega dell'esplorazione nelle istruzioni di `get_task`: ai task che
//! devono leggere molto codice senza che serva tenerlo a mente (revisione,
//! ricerca, correzione di bug, refactoring, lavoro generico) si aggiunge il
//! paragrafo che spiega come affidare i giri larghi all'esploratore
//! economico dell'agente, se l'agente ne ha uno (`ExplorationDelegation`).

use crate::agents::registry;
use crate::board::{texts, Task, TaskKind};
use crate::bots::SkillRef;
use serde_json::Value;

/// Skill del plugin il cui task `plan` e' una ricerca sul codice.
const RESEARCH_SKILL: &str = "code-research";
/// Skill del plugin i cui task `work` leggono molto codice prima di cambiarlo.
const WORK_SKILLS: [&str; 2] = ["bug-fix", "safe-refactor"];

/// Il task ha bisogno di esplorare il repository in largo? Revisione sempre;
/// pianificazione e lavoro solo senza skill o con una di quelle sopra. Le
/// skill dell'utente e le altre del plugin non ricevono il paragrafo.
fn qualifies(kind: TaskKind, skill: Option<&SkillRef>) -> bool {
    match (kind, skill) {
        (TaskKind::Review, _) => true,
        (_, None) => true,
        (TaskKind::Plan, Some(SkillRef::Plugin(id))) => id == RESEARCH_SKILL,
        (TaskKind::Work, Some(SkillRef::Plugin(id))) => WORK_SKILLS.contains(&id.as_str()),
        (_, Some(SkillRef::User(_))) => false,
    }
}

/// Accoda alle istruzioni di `get_task` il paragrafo della delega, se il task
/// ne ha bisogno e il suo agente dichiara `exploration_delegation`.
pub(super) fn add_to_task_json(out: &mut Value, task: &Task) {
    if !qualifies(task.kind, task.skill.as_ref()) {
        return;
    }
    let Some(delegation) = registry::find(&task.agent).ok().and_then(|i| i.capabilities().exploration_delegation)
    else {
        return;
    };
    if let Some(instructions) = out["instructions"].as_str() {
        out["instructions"] =
            format!("{instructions}\n\n{}", texts::exploration_delegation(&delegation.explorer_invocation())).into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(id: &str) -> Option<SkillRef> {
        Some(SkillRef::Plugin(id.into()))
    }

    #[test]
    fn a_review_always_qualifies() {
        for skill in [None, plugin("code-research"), plugin("test-qa"), Some(SkillRef::User("mine".into()))] {
            assert!(qualifies(TaskKind::Review, skill.as_ref()), "{skill:?}");
        }
    }

    #[test]
    fn a_plan_qualifies_without_skill_or_with_code_research() {
        assert!(qualifies(TaskKind::Plan, None));
        assert!(qualifies(TaskKind::Plan, plugin("code-research").as_ref()));
        for id in ["code-answer", "bug-fix", "safe-refactor", "task-intake"] {
            assert!(!qualifies(TaskKind::Plan, plugin(id).as_ref()), "{id}");
        }
        assert!(!qualifies(TaskKind::Plan, Some(&SkillRef::User("code-research".into()))));
    }

    #[test]
    fn a_work_task_qualifies_without_skill_or_with_bug_fix_or_safe_refactor() {
        assert!(qualifies(TaskKind::Work, None));
        for id in ["bug-fix", "safe-refactor"] {
            assert!(qualifies(TaskKind::Work, plugin(id).as_ref()), "{id}");
        }
        for id in ["test-qa", "docs-writer", "git-ops", "release-prep", "code-research", "code-answer"] {
            assert!(!qualifies(TaskKind::Work, plugin(id).as_ref()), "{id}");
        }
        assert!(!qualifies(TaskKind::Work, Some(&SkillRef::User("bug-fix".into()))));
    }
}
