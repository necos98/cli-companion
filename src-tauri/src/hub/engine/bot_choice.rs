//! I Bot come li vede un segretario (puro): l'elenco nel system prompt di chi
//! apre o modifica lavoro sulla board (`open_plan_task`, `request_task_update`)
//! e la verifica del Bot scelto, prima di toccare la board. Testi in inglese:
//! li legge il modello. Il resto della risoluzione (agente, modello, skill, e il
//! tipo del task di una modifica) e' della board (`board::resolve_spec`).

use crate::board::TaskKind;
use crate::bots::AgentBot;
use crate::hub::settings::{Secretary, SecretaryTool};
use crate::hub::text::clean_line;

/// Caratteri tenuti del nome di un Bot nel contesto del modello (`bots::validate` ne ammette 24).
const NAME_KEPT: usize = 40;
/// Caratteri tenuti del compito di un Bot (`bots::validate` ne ammette 300).
const TASK_KEPT: usize = 320;
/// Caratteri tenuti dell'id scelto dal modello, nei messaggi d'errore.
const ID_KEPT: usize = 64;

const INTRO: &str = "Bots: a bot fixes the coding agent, the model, the effort and the skill of a task, and runs \
only the kinds of tasks it allows. Choose the bot whose task fits the work.";
const PLAN_INTRO: &str = "Every planning task you open (`open_plan_task`) runs with one of the bots that can plan \
(id: name - task):";
const NO_PLAN_BOTS: &str = "No enabled bot can run planning tasks now, so you cannot open planning tasks: tell the \
user to enable a bot whose kinds include plan in the settings.";
const UPDATE_INTRO: &str = "A task's bot can be changed with `request_task_update`, only to a bot whose kinds include \
the kind of that task (work, review or plan). The bots that can run tasks (id: name - task [kinds]):";
const NO_TASK_BOTS: &str = "No enabled bot can run tasks now, so you cannot change the bot of a task: tell the user \
to enable a bot in the settings.";

/// La sezione dei Bot del system prompt, solo per un segretario che usa un tool
/// che li chiede: per `open_plan_task` i Bot che pianificano, per
/// `request_task_update` quelli che eseguono un tipo di task almeno, con i loro
/// tipi. I Bot disabilitati non si elencano.
pub fn describe(secretary: &Secretary, bots: &[AgentBot]) -> Option<String> {
    let opens_plans = secretary.may_use(SecretaryTool::OpenPlanTask);
    let updates_tasks = secretary.may_use(SecretaryTool::RequestTaskUpdate);
    if !opens_plans && !updates_tasks {
        return None;
    }
    let enabled: Vec<&AgentBot> = bots.iter().filter(|b| b.enabled).collect();
    let mut sections = vec![INTRO.to_string()];
    if opens_plans {
        let planners: Vec<String> = enabled.iter().filter(|b| b.runs(TaskKind::Plan)).map(|b| line(b)).collect();
        sections.push(listing(PLAN_INTRO, NO_PLAN_BOTS, &planners));
    }
    if updates_tasks {
        let runners: Vec<String> = enabled
            .iter()
            .filter(|b| !b.kinds.is_empty())
            .map(|b| format!("{} [{}]", line(b), kinds_label(&b.kinds)))
            .collect();
        sections.push(listing(UPDATE_INTRO, NO_TASK_BOTS, &runners));
    }
    Some(sections.join("\n"))
}

/// `- id: nome - compito`, ripuliti e accorciati.
fn line(bot: &AgentBot) -> String {
    format!("- {}: {} - {}", bot.id, clean_line(&bot.name, NAME_KEPT), clean_line(&bot.task, TASK_KEPT))
}

fn kinds_label(kinds: &[TaskKind]) -> String {
    kinds.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ")
}

/// L'introduzione con le righe, o il testo per l'elenco vuoto.
fn listing(intro: &str, empty: &str, lines: &[String]) -> String {
    if lines.is_empty() {
        empty.to_string()
    } else {
        format!("{intro}\n{}", lines.join("\n"))
    }
}

/// Il Bot `id` esiste, e' abilitato e puo' eseguire i task `kind`; altrimenti
/// l'errore per il modello, senza ripieghi su un altro Bot.
pub fn check(bots: &[AgentBot], id: &str, kind: TaskKind) -> Result<(), String> {
    let bot = find_available(bots, id)?;
    if bot.runs(kind) {
        return Ok(());
    }
    let kind = kind.as_str();
    Err(format!(
        "The bot \"{}\" cannot run {kind} tasks: choose one of the bots listed in your instructions whose kinds \
        include {kind}, or ask the user to allow {kind} tasks for a bot in the settings.",
        clean_line(id, ID_KEPT)
    ))
}

/// Il Bot `id` esiste ed e' abilitato: per il cambio di Bot di un task, il cui
/// tipo lo verifica la board.
pub fn check_available(bots: &[AgentBot], id: &str) -> Result<(), String> {
    find_available(bots, id).map(drop)
}

fn find_available<'b>(bots: &'b [AgentBot], id: &str) -> Result<&'b AgentBot, String> {
    let shown = clean_line(id, ID_KEPT);
    match bots.iter().find(|b| b.id == id) {
        None => Err(format!("Unknown bot \"{shown}\": use the id of one of the bots listed in your instructions.")),
        Some(bot) if !bot.enabled => Err(format!(
            "The bot \"{shown}\" is disabled: choose another of the bots listed in your instructions, or ask the \
            user to enable it in the settings."
        )),
        Some(bot) => Ok(bot),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::default_agent_bots;
    use crate::hub::engine::fixtures::secretary;

    fn bots() -> Vec<AgentBot> {
        default_agent_bots()
    }

    fn with_tools(tools: Vec<SecretaryTool>) -> Secretary {
        Secretary { tools, ..secretary("mario") }
    }

    fn by_id<'a>(list: &'a mut [AgentBot], id: &str) -> &'a mut AgentBot {
        list.iter_mut().find(|b| b.id == id).unwrap()
    }

    #[test]
    fn a_planning_secretary_sees_only_the_enabled_bots_that_plan() {
        let mut list = bots();
        by_id(&mut list, "general").kinds.push(TaskKind::Plan);
        by_id(&mut list, "skill-wizard").kinds = vec![TaskKind::Plan];
        by_id(&mut list, "skill-wizard").enabled = false;
        let text = describe(&with_tools(vec![SecretaryTool::OpenPlanTask]), &list).unwrap();
        assert_eq!(
            text,
            format!(
                "{INTRO}\n{PLAN_INTRO}\n- general: General - General-purpose coding agent: new features and any \
                 change no specialized bot covers.\n- planner: Planner - Plans changes to the project: explores the \
                 repository read-only and proposes queue tasks.\n- quick-answer: Quick Answer - Answers \
                 a quick question about the code or its git history, read-only, in a few lines. Changes nothing and \
                 proposes nothing.\n- researcher: Researcher - Researches a technical question in depth, read-only: \
                 options, feasibility, risks and effort, in a structured report. May propose tasks only if the user \
                 asked for them."
            )
        );
    }

    #[test]
    fn an_updating_secretary_sees_the_bots_that_run_tasks_with_their_kinds() {
        let mut list = bots();
        by_id(&mut list, "general").kinds = vec![TaskKind::Work, TaskKind::Review, TaskKind::Plan];
        by_id(&mut list, "reviewer").enabled = false;
        let text = describe(&with_tools(vec![SecretaryTool::RequestTaskUpdate]), &list).unwrap();
        assert!(text.starts_with(&format!("{INTRO}\n{UPDATE_INTRO}\n")), "{text}");
        assert!(
            text.contains(
                "\n- general: General - General-purpose coding agent: new features and any change no \
                 specialized bot covers. [work, review, plan]"
            ),
            "{text}"
        );
        assert!(text.contains(
            "\n- planner: Planner - Plans changes to the project: explores the repository \
                 read-only and proposes queue tasks. [plan]"
        ));
        for hidden in ["intake", "setup", "skill-wizard", "reviewer"] {
            assert!(!text.contains(&format!("- {hidden}:")), "{hidden}: {text}");
        }
        assert!(!text.contains(PLAN_INTRO), "{text}");
    }

    #[test]
    fn a_secretary_with_both_tools_sees_both_listings() {
        let text = describe(&with_tools(vec![SecretaryTool::OpenPlanTask, SecretaryTool::RequestTaskUpdate]), &bots());
        let text = text.unwrap();
        assert!(text.find(PLAN_INTRO).unwrap() < text.find(UPDATE_INTRO).unwrap(), "{text}");
    }

    #[test]
    fn only_a_secretary_that_opens_or_changes_work_sees_the_bots() {
        let reader = with_tools(vec![SecretaryTool::BoardStatus, SecretaryTool::PostMessage]);
        assert_eq!(describe(&reader, &bots()), None);
    }

    #[test]
    fn without_bots_that_can_do_the_work_the_secretary_is_told_so() {
        let mut list = bots();
        by_id(&mut list, "planner").enabled = false;
        by_id(&mut list, "quick-answer").enabled = false;
        by_id(&mut list, "researcher").enabled = false;
        let both = with_tools(vec![SecretaryTool::OpenPlanTask, SecretaryTool::RequestTaskUpdate]);
        let text = describe(&both, &list).unwrap();
        assert!(text.contains(NO_PLAN_BOTS) && text.contains(UPDATE_INTRO), "{text}");
        list.iter_mut().for_each(|b| b.enabled = false);
        assert_eq!(describe(&both, &list), Some(format!("{INTRO}\n{NO_PLAN_BOTS}\n{NO_TASK_BOTS}")));
    }

    #[test]
    fn an_unknown_or_disabled_bot_is_refused_without_falling_back() {
        let mut list = bots();
        assert_eq!(check(&list, "planner", TaskKind::Plan), Ok(()));
        assert_eq!(check_available(&list, "intake"), Ok(()));
        let unknown = check(&list, "nope", TaskKind::Plan).unwrap_err();
        assert!(unknown.starts_with("Unknown bot \"nope\""), "{unknown}");
        assert_eq!(check_available(&list, "nope").unwrap_err(), unknown);
        by_id(&mut list, "planner").enabled = false;
        let disabled = check(&list, "planner", TaskKind::Plan).unwrap_err();
        assert!(disabled.starts_with("The bot \"planner\" is disabled"), "{disabled}");
    }

    /// Git Operator e Release nascono spenti: il segretario non li vede e non li
    /// sceglie finche' l'utente non li accende.
    #[test]
    fn the_preset_bots_that_start_disabled_are_hidden_and_refused_until_enabled() {
        let mut list = bots();
        let both = with_tools(vec![SecretaryTool::OpenPlanTask, SecretaryTool::RequestTaskUpdate]);
        let text = describe(&both, &list).unwrap();
        for id in ["git-operator", "release"] {
            assert!(!text.contains(&format!("- {id}:")), "{id}: {text}");
            let refused = check(&list, id, TaskKind::Work).unwrap_err();
            assert!(refused.starts_with(&format!("The bot \"{id}\" is disabled")), "{refused}");
        }
        by_id(&mut list, "git-operator").enabled = true;
        assert!(describe(&both, &list).unwrap().contains("\n- git-operator: Git Operator - Performs git maintenance"));
        assert_eq!(check(&list, "git-operator", TaskKind::Work), Ok(()));
    }

    #[test]
    fn a_bot_that_cannot_run_the_kind_is_refused() {
        let err = check(&bots(), "general", TaskKind::Plan).unwrap_err();
        assert!(err.starts_with("The bot \"general\" cannot run plan tasks"), "{err}");
        assert!(err.contains("whose kinds include plan"), "{err}");
    }
}
