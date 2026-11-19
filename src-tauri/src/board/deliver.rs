//! Task di consegna (`kind: "deliver"`): porta il branch del goal sulla base
//! per intero, come dice la modalita' di consegna del workspace (docs/GOALS.MD,
//! "Consegna del goal"). Come `plan` lo crea solo l'app (`Board::create_deliver_task`),
//! mai una proposta o un task manuale. Un Bot lo esegue se esegue i `work`.
//! Funzioni pure.

use super::review::reviewed_entry;
use super::{Board, ReportFields, TaskKind};
use serde_json::Value;

/// Errore di un `kind: "deliver"` arrivato da fuori (proposta, task manuale, modifica).
pub const DELIVER_KIND_RESERVED: &str =
    "kind \"deliver\" is reserved to the app: delivery tasks cannot be proposed, created or set by hand";

/// Errore di `commits` o `pr_urls` nel report di un task che non e' di consegna.
const DELIVER_REPORT_ONLY: &str = "commits and pr_urls are only accepted for deliver tasks";

impl TaskKind {
    /// Il tipo che un Bot deve avere fra i suoi `kinds` per eseguire il task:
    /// un `deliver` lo esegue chi esegue i `work`, gli altri tipi se stessi.
    pub fn bot_kind(self) -> TaskKind {
        match self {
            TaskKind::Deliver => TaskKind::Work,
            other => other,
        }
    }
}

/// `commits` e `pr_urls` (uno per repository) sono propri dei task di consegna.
pub fn check_deliver_report(kind: TaskKind, f: &ReportFields) -> Result<(), String> {
    if kind != TaskKind::Deliver && (!f.commits.is_empty() || !f.pr_urls.is_empty()) {
        return Err(DELIVER_REPORT_ONLY.to_string());
    }
    Ok(())
}

/// I task `work` e `review` del goal `goal_id` in ordine di coda, nella forma di
/// `reviewed_tasks` (report senza il testo della specifica): `goal_tasks` di
/// `get_task` del worker di consegna, che completa con `get_task_details`.
pub fn goal_task_entries(board: &Board, goal_id: &str) -> Value {
    let wanted = |k: TaskKind| matches!(k, TaskKind::Work | TaskKind::Review);
    Value::Array(board.goal_tasks(goal_id).filter(|t| wanted(t.kind)).map(reviewed_entry).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{check_kind_input, check_verdict};

    #[test]
    fn a_bot_runs_a_delivery_as_a_work_task() {
        assert_eq!(TaskKind::Deliver.bot_kind(), TaskKind::Work);
        for k in [TaskKind::Work, TaskKind::Review, TaskKind::Plan] {
            assert_eq!(k.bot_kind(), k);
        }
        assert_eq!(TaskKind::parse("deliver"), Some(TaskKind::Deliver));
        assert_eq!(TaskKind::Deliver.as_str(), "deliver");
    }

    #[test]
    fn deliver_is_never_an_input_and_a_delivery_keeps_its_kind() {
        for base in [None, Some(TaskKind::Work), Some(TaskKind::Review)] {
            assert_eq!(check_kind_input(TaskKind::Deliver, base), Err(DELIVER_KIND_RESERVED.to_string()), "{base:?}");
        }
        assert_eq!(check_kind_input(TaskKind::Deliver, Some(TaskKind::Deliver)), Ok(()));
        let plan = check_kind_input(TaskKind::Deliver, Some(TaskKind::Plan)).unwrap_err();
        assert!(plan.contains("plan task cannot be changed"), "{plan}");
        for other in [TaskKind::Work, TaskKind::Review, TaskKind::Plan] {
            let e = check_kind_input(other, Some(TaskKind::Deliver)).unwrap_err();
            assert!(e.contains("delivery task cannot be changed"), "{e}");
        }
    }

    #[test]
    fn only_a_delivery_reports_commits_and_pr_urls() {
        let with_commits = ReportFields { commits: vec!["abc".into()], ..ReportFields::default() };
        let with_prs = ReportFields { pr_urls: vec!["https://x/pull/1".into()], ..ReportFields::default() };
        for f in [&with_commits, &with_prs] {
            assert_eq!(check_deliver_report(TaskKind::Deliver, f), Ok(()));
            for k in [TaskKind::Work, TaskKind::Review, TaskKind::Plan] {
                assert_eq!(check_deliver_report(k, f), Err(DELIVER_REPORT_ONLY.to_string()), "{k:?}");
            }
        }
        assert_eq!(check_deliver_report(TaskKind::Work, &ReportFields::default()), Ok(()));
    }

    #[test]
    fn a_delivery_carries_no_verdict_or_findings() {
        let verdict = ReportFields { findings: vec!["x".into()], ..ReportFields::default() };
        let e = check_verdict(TaskKind::Deliver, true, &verdict).unwrap_err();
        assert!(e.contains("only accepted for review tasks"), "{e}");
        assert_eq!(check_verdict(TaskKind::Deliver, true, &ReportFields::default()), Ok(()));
    }
}
