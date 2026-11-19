//! Validazione dei campi di un Bot (e dei nomi della pool): il frontend non e'
//! fidato. Pura: gli agenti arrivano con `AgentCatalog`. Gli errori sono in
//! italiano (li mostra l'interfaccia). Unicita' dei nomi e degli id: `agent_bots`.

use super::agent::{AgentBotDraft, ModelChoice, SkillRef};
use super::catalog::AgentCatalog;
use super::image::validate_image;
use crate::agents::presets::PRESETS as PLUGIN_SKILLS;
use crate::agents::{effort_is_supported, model_is_valid, AgentIntegration, AgentUnavailable};
use crate::board::TaskKind;

pub const NAME_MAX_CHARS: usize = 24;
pub const TASK_MAX_CHARS: usize = 300;

/// Nome di un Bot o della pool: trim, 1-24 caratteri, niente caratteri di controllo.
pub fn normalize_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    let chars = name.chars().count();
    if chars == 0 || chars > NAME_MAX_CHARS {
        return Err(format!("il nome deve avere da 1 a {NAME_MAX_CHARS} caratteri"));
    }
    if name.chars().any(char::is_control) {
        return Err(format!("il nome “{name}” contiene caratteri non validi"));
    }
    Ok(name.to_string())
}

/// I campi in forma canonica, o il primo errore.
pub fn validate_draft(draft: AgentBotDraft, catalog: &AgentCatalog) -> Result<AgentBotDraft, String> {
    let agent = draft.agent.as_deref().map(|id| catalog.find(id).map_err(unavailable)).transpose()?;
    let model = validate_model(draft.model, agent)?;
    if let (Some(effort), Some(agent)) = (draft.effort, agent) {
        if !effort_is_supported(agent, effort) {
            return Err(format!("{} non supporta l'effort “{}”", agent.display_name(), effort.as_str()));
        }
    }
    let image = draft.image.map(|img| validate_image(&img.png, &img.color)).transpose()?;
    Ok(AgentBotDraft {
        name: normalize_name(&draft.name)?,
        task: normalize_task(&draft.task)?,
        agent: agent.map(|a| a.id().to_string()),
        model,
        effort: draft.effort,
        skill: draft.skill.map(validate_skill).transpose()?,
        kinds: normalize_kinds(draft.kinds),
        enabled: draft.enabled,
        image,
    })
}

/// I tipi di task senza doppioni, nell'ordine di `TaskKind::ALL`. Il tipo `deliver` non
/// si sceglie: un Bot che esegue i `work` esegue anche le consegne dei goal (vedi
/// `TaskKind::bot_kind`), quindi si scarta.
pub fn normalize_kinds(kinds: Vec<TaskKind>) -> Vec<TaskKind> {
    TaskKind::ALL.into_iter().filter(|k| *k != TaskKind::Deliver && kinds.contains(k)).collect()
}

fn unavailable(reason: AgentUnavailable) -> String {
    match reason {
        AgentUnavailable::Unknown(id) => format!("agente sconosciuto: {id}"),
        AgentUnavailable::Disabled { name, .. } => format!("{name} è disattivato: abilitalo nelle integrazioni"),
        AgentUnavailable::MissingContract { name, .. } => format!("{name} non si può usare per un Bot"),
    }
}

/// Il compito: trim, 1-300 caratteri su una riga.
fn normalize_task(raw: &str) -> Result<String, String> {
    let task = raw.trim();
    let chars = task.chars().count();
    if chars == 0 || chars > TASK_MAX_CHARS {
        return Err(format!("il compito deve avere da 1 a {TASK_MAX_CHARS} caratteri"));
    }
    if task.chars().any(char::is_control) {
        return Err("il compito deve stare su una riga, senza caratteri di controllo".into());
    }
    Ok(task.to_string())
}

/// Un modello concreto vale solo per l'agente fissato, che deve riconoscerlo.
fn validate_model(model: ModelChoice, agent: Option<&dyn AgentIntegration>) -> Result<ModelChoice, String> {
    let ModelChoice::Id(raw) = model else {
        return Ok(model);
    };
    let id = raw.trim();
    let agent = agent.ok_or("per scegliere un modello preciso fissa prima l'agente del Bot")?;
    if !model_is_valid(agent, id) {
        return Err(format!("modello non valido per {}: “{id}”", agent.display_name()));
    }
    Ok(ModelChoice::Id(id.to_string()))
}

/// Skill del plugin esistente; skill utente con un nome valido (l'esistenza si
/// verifica al lancio).
fn validate_skill(skill: SkillRef) -> Result<SkillRef, String> {
    match skill {
        SkillRef::Plugin(id) if PLUGIN_SKILLS.iter().any(|p| p.id == id) => Ok(SkillRef::Plugin(id)),
        SkillRef::Plugin(id) => Err(format!("skill del plugin sconosciuta: “{id}”")),
        SkillRef::User(name) => {
            let name = name.trim().to_string();
            crate::user_skills::validate_name(&name).map_err(|_| format!("nome di skill non valido: “{name}”"))?;
            Ok(SkillRef::User(name))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::testing::{FakeIntegration, FAKE_MODEL_LARGE};
    use crate::agents::{Capability, Effort, ModelTier};
    use crate::bots::BotImage;

    const FULL: &str = "fake-full";
    const PLAIN: &str = "fake-plain";
    const OFF: &str = "fake-off";

    fn with_catalog<R>(f: impl FnOnce(&AgentCatalog) -> R) -> R {
        let full = FakeIntegration::full(FULL);
        let plain = FakeIntegration::full(PLAIN).without(Capability::Effort);
        let off = FakeIntegration::full(OFF);
        let registered = [&full as &dyn AgentIntegration, &plain, &off];
        f(&AgentCatalog::new(&registered, vec![FULL.into(), PLAIN.into()]))
    }

    fn draft() -> AgentBotDraft {
        AgentBotDraft {
            name: "Reviewer".into(),
            task: "Reviews diffs.".into(),
            agent: None,
            model: ModelChoice::Default,
            effort: None,
            skill: None,
            kinds: vec![TaskKind::Work],
            enabled: true,
            image: None,
        }
    }

    fn check(d: AgentBotDraft) -> Result<AgentBotDraft, String> {
        with_catalog(|c| validate_draft(d, c))
    }

    #[test]
    fn a_minimal_draft_is_valid_and_trimmed() {
        let ok = check(AgentBotDraft { name: "  Reviewer ".into(), task: " Reviews diffs. ".into(), ..draft() });
        assert_eq!(ok, Ok(draft()));
    }

    #[test]
    fn names_follow_the_pool_rules() {
        assert!(normalize_name(&"é".repeat(NAME_MAX_CHARS)).is_ok());
        for bad in ["", "   ", "a\nb", "a\u{0}b"] {
            assert!(normalize_name(bad).is_err(), "{bad:?}");
        }
        assert!(normalize_name(&"a".repeat(NAME_MAX_CHARS + 1)).is_err());
        assert!(check(AgentBotDraft { name: " ".into(), ..draft() }).is_err());
    }

    #[test]
    fn the_task_is_one_non_empty_line_within_the_cap() {
        assert!(check(AgentBotDraft { task: "x".repeat(TASK_MAX_CHARS), ..draft() }).is_ok());
        for bad in [String::new(), "  ".into(), "a\nb".into(), "x".repeat(TASK_MAX_CHARS + 1)] {
            assert!(check(AgentBotDraft { task: bad.clone(), ..draft() }).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_fixed_agent_must_exist_and_be_enabled() {
        assert_eq!(check(AgentBotDraft { agent: Some(FULL.into()), ..draft() }).unwrap().agent.as_deref(), Some(FULL));
        assert_eq!(
            check(AgentBotDraft { agent: Some("gone".into()), ..draft() }).unwrap_err(),
            "agente sconosciuto: gone"
        );
        let off = check(AgentBotDraft { agent: Some(OFF.into()), ..draft() }).unwrap_err();
        assert!(off.contains("disattivato"), "{off}");
    }

    #[test]
    fn a_model_id_needs_a_fixed_agent_that_knows_it() {
        let id = |model: &str, agent: Option<&str>| AgentBotDraft {
            model: ModelChoice::Id(model.into()),
            agent: agent.map(str::to_string),
            ..draft()
        };
        assert_eq!(
            check(id(&format!(" {FAKE_MODEL_LARGE} "), Some(FULL))).unwrap().model,
            id(FAKE_MODEL_LARGE, None).model
        );
        assert!(check(id(FAKE_MODEL_LARGE, None)).unwrap_err().contains("fissa prima l'agente"));
        assert!(check(id("other-model", Some(FULL))).is_err());
        assert!(check(id("", Some(FULL))).is_err());
        // Le fasce e il predefinito valgono con qualunque agente.
        for model in [ModelChoice::Default, ModelChoice::Tier(ModelTier::Strong)] {
            assert!(check(AgentBotDraft { model: model.clone(), ..draft() }).is_ok());
            assert!(check(AgentBotDraft { model, agent: Some(PLAIN.into()), ..draft() }).is_ok());
        }
    }

    #[test]
    fn effort_must_be_supported_by_a_fixed_agent_only() {
        let effort = |level, agent: Option<&str>| AgentBotDraft {
            effort: Some(level),
            agent: agent.map(str::to_string),
            ..draft()
        };
        assert!(check(effort(Effort::High, Some(FULL))).is_ok());
        assert!(check(effort(Effort::Max, Some(FULL))).is_err());
        assert!(check(effort(Effort::Low, Some(PLAIN))).is_err());
        // Con l'agente del workspace si ammette: al lancio, se non e' supportato, si ignora con un avviso.
        assert!(check(effort(Effort::Max, None)).is_ok());
    }

    #[test]
    fn skills_are_checked_by_kind() {
        let skill = |s: SkillRef| AgentBotDraft { skill: Some(s), ..draft() };
        assert!(check(skill(SkillRef::Plugin("task-intake".into()))).is_ok());
        assert!(check(skill(SkillRef::Plugin("nope".into()))).is_err());
        assert_eq!(
            check(skill(SkillRef::User(" my-skill ".into()))).unwrap().skill,
            Some(SkillRef::User("my-skill".into()))
        );
        for bad in ["My Skill", "../x", "", "task-intake"] {
            assert!(check(skill(SkillRef::User(bad.into()))).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn kinds_are_deduplicated_in_canonical_order() {
        let kinds = |k: Vec<TaskKind>| check(AgentBotDraft { kinds: k, ..draft() }).unwrap().kinds;
        let all = vec![TaskKind::Work, TaskKind::Review, TaskKind::Plan];
        assert_eq!(kinds(vec![TaskKind::Plan, TaskKind::Work, TaskKind::Plan, TaskKind::Review]), all);
        assert_eq!(kinds(vec![TaskKind::Review, TaskKind::Review]), [TaskKind::Review]);
        // `deliver` non si sceglie: lo esegue chi esegue i `work`.
        assert_eq!(kinds(TaskKind::ALL.to_vec()), all);
        assert_eq!(kinds(vec![TaskKind::Deliver, TaskKind::Work]), [TaskKind::Work]);
        assert!(kinds(vec![TaskKind::Deliver]).is_empty());
        // Nessun tipo = Bot solo interattivo: ammesso.
        assert!(kinds(Vec::new()).is_empty());
    }

    #[test]
    fn the_image_is_validated() {
        let bad = AgentBotDraft { image: Some(BotImage { png: "AAAA".into(), color: "#112233".into() }), ..draft() };
        assert!(check(bad).is_err());
    }
}
