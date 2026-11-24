//! I Bot predefiniti: tutti partono con l'agente del workspace. Aggiungerne uno
//! = una voce in `PRESETS` + una migrazione additiva di `bots.json` che lo crea
//! (vedi `migrations`): i file esistenti lo ricevono una volta sola e un Bot
//! cancellato non ricompare. L'id e' stabile: lo ricorda il Bot creato
//! (`AgentBot::preset`) e permette di ripristinarlo. Gli id dei Bot creati
//! dall'utente cominciano con `USER_ID_PREFIX`, che nessun preset usa.

use super::agent::{AgentBot, ModelChoice, SkillRef};
use crate::agents::{Effort, ModelTier};
use crate::board::TaskKind;

/// Il Bot generico, obbligatorio: configurabile ma non cancellabile ne' disattivabile.
pub const GENERAL_ID: &str = "general";

/// Prefisso degli id dei Bot creati dall'utente.
pub const USER_ID_PREFIX: &str = "custom-";

/// Un Bot predefinito.
pub struct BotPreset {
    pub id: &'static str,
    pub name: &'static str,
    /// Il compito, in inglese: lo legge il modello.
    pub task: &'static str,
    pub model: ModelChoice,
    pub effort: Option<Effort>,
    /// Id di una skill del plugin (`agents::presets::PRESETS`).
    pub skill: Option<&'static str>,
    /// I tipi di task automatici che esegue; vuoto = solo interattivo.
    pub kinds: &'static [TaskKind],
    /// Acceso alla creazione (file nuovo, migrazione, ripristino). Spento per i
    /// Bot con effetti fuori dal flusso di consegna: l'utente li accende di proposito.
    pub enabled_by_default: bool,
}

impl BotPreset {
    /// Il Bot appena creato da questo preset: acceso se `enabled_by_default`, senza immagine.
    pub fn bot(&self) -> AgentBot {
        AgentBot {
            id: self.id.to_string(),
            name: self.name.to_string(),
            task: self.task.to_string(),
            agent: None,
            model: self.model.clone(),
            effort: self.effort,
            skill: self.skill(),
            kinds: self.kinds.to_vec(),
            enabled: self.enabled_by_default,
            image: None,
            preset: Some(self.id.to_string()),
        }
    }

    pub fn skill(&self) -> Option<SkillRef> {
        self.skill.map(|id| SkillRef::Plugin(id.to_string()))
    }
}

/// I preset, nell'ordine in cui compaiono.
pub static PRESETS: [BotPreset; 14] = [
    BotPreset {
        id: GENERAL_ID,
        name: "General",
        task: "General-purpose coding agent: new features and any change no specialized bot covers.",
        model: ModelChoice::Default,
        effort: None,
        skill: None,
        kinds: &[TaskKind::Work],
        enabled_by_default: true,
    },
    BotPreset {
        id: "intake",
        name: "Intake",
        task: "Turns requests into queue tasks.",
        model: ModelChoice::Tier(ModelTier::Strong),
        effort: Some(Effort::High),
        skill: Some("task-intake"),
        kinds: &[],
        enabled_by_default: true,
    },
    BotPreset {
        id: "setup",
        name: "Setup",
        task: "Configures the workspace and the worker rules.",
        model: ModelChoice::Tier(ModelTier::Standard),
        effort: Some(Effort::Medium),
        skill: Some("project-setup"),
        kinds: &[],
        enabled_by_default: true,
    },
    BotPreset {
        id: "skill-wizard",
        name: "Skill Wizard",
        task: "Creates and manages global skills.",
        model: ModelChoice::Tier(ModelTier::Standard),
        effort: Some(Effort::Medium),
        skill: Some("skill-wizard"),
        kinds: &[],
        enabled_by_default: true,
    },
    BotPreset {
        id: "reviewer",
        name: "Reviewer",
        task: "Verifies the integrated result of finished tasks and proposes fixes.",
        model: ModelChoice::Tier(ModelTier::Strong),
        effort: Some(Effort::High),
        skill: None,
        kinds: &[TaskKind::Review],
        enabled_by_default: true,
    },
    BotPreset {
        id: "planner",
        name: "Planner",
        task: "Plans changes to the project: explores the repository read-only and proposes queue tasks.",
        model: ModelChoice::Tier(ModelTier::Strong),
        effort: Some(Effort::High),
        skill: None,
        kinds: &[TaskKind::Plan],
        enabled_by_default: true,
    },
    BotPreset {
        id: "quick-answer",
        name: "Quick Answer",
        task: "Answers a quick question about the code or its git history, read-only, in a few lines. Changes nothing \
               and proposes nothing.",
        model: ModelChoice::Tier(ModelTier::Standard),
        effort: Some(Effort::Low),
        skill: Some("code-answer"),
        kinds: &[TaskKind::Plan],
        enabled_by_default: true,
    },
    BotPreset {
        id: "bug-fixer",
        name: "Bug Fixer",
        task: "Fixes a reported bug: reproduces it, adds a failing regression test, fixes the root cause and verifies.",
        model: ModelChoice::Tier(ModelTier::Strong),
        effort: Some(Effort::High),
        skill: Some("bug-fix"),
        kinds: &[TaskKind::Work],
        enabled_by_default: true,
    },
    BotPreset {
        id: "researcher",
        name: "Researcher",
        task: "Researches a technical question in depth, read-only: options, feasibility, risks and effort, in a \
               structured report. May propose tasks only if the user asked for them.",
        model: ModelChoice::Tier(ModelTier::Strong),
        effort: Some(Effort::High),
        skill: Some("code-research"),
        kinds: &[TaskKind::Plan],
        enabled_by_default: true,
    },
    BotPreset {
        id: "test-writer",
        name: "Test Writer",
        task: "Adds or improves automated tests for existing behaviour, following the project's test conventions; \
               changes production code only to make it testable.",
        model: ModelChoice::Tier(ModelTier::Standard),
        effort: Some(Effort::Medium),
        skill: Some("test-qa"),
        kinds: &[TaskKind::Work],
        enabled_by_default: true,
    },
    BotPreset {
        id: "docs-writer",
        name: "Docs Writer",
        task: "Writes or updates documentation (README, guides, comments) for the stated audience and language; does \
               not change behaviour.",
        model: ModelChoice::Tier(ModelTier::Standard),
        effort: Some(Effort::Medium),
        skill: Some("docs-writer"),
        kinds: &[TaskKind::Work],
        enabled_by_default: true,
    },
    BotPreset {
        id: "refactorer",
        name: "Refactorer",
        task: "Restructures existing code without changing its behaviour, in small verified steps, with tests run \
               before and after.",
        model: ModelChoice::Tier(ModelTier::Strong),
        effort: Some(Effort::High),
        skill: Some("safe-refactor"),
        kinds: &[TaskKind::Work],
        enabled_by_default: true,
    },
    BotPreset {
        id: "git-operator",
        name: "Git Operator",
        task: "Performs git maintenance asked by the user (branch cleanup, conflict resolution, reverting a change) \
               safely: never rewrites shared history or force-pushes.",
        model: ModelChoice::Tier(ModelTier::Strong),
        effort: Some(Effort::Medium),
        skill: Some("git-ops"),
        kinds: &[TaskKind::Work],
        enabled_by_default: false,
    },
    BotPreset {
        id: "release",
        name: "Release",
        task: "Prepares a release: version bump, changelog and release notes from the merged work; never publishes, \
               tags remotely or deploys without the user's explicit confirmation.",
        model: ModelChoice::Tier(ModelTier::Strong),
        effort: Some(Effort::High),
        skill: Some("release-prep"),
        kinds: &[TaskKind::Work],
        enabled_by_default: false,
    },
];

/// Il preset con quell'id.
pub fn find(id: &str) -> Option<&'static BotPreset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// Tutti i Bot predefiniti, come in un file nuovo.
pub fn default_bots() -> Vec<AgentBot> {
    PRESETS.iter().map(BotPreset::bot).collect()
}

/// Il Bot `general` e' sempre presente e acceso: se manca (file scritto a mano)
/// torna in testa all'elenco.
pub fn ensure_general(bots: &mut Vec<AgentBot>) {
    match bots.iter_mut().find(|b| b.id == GENERAL_ID) {
        Some(general) => general.enabled = true,
        None => bots.insert(0, PRESETS[0].bot()),
    }
}

#[cfg(test)]
mod tests;
