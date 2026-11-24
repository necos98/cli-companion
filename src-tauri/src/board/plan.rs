//! Task di pianificazione (`kind: "plan"`): un worker esplora il repository in
//! sola lettura, scrive le specifiche e chiude con una proposta di task (o con
//! le domande per l'utente). Non consegna codice: niente commit, branch da
//! integrare o istruzioni di consegna. Lo crea solo l'app (vedi
//! `automation::planner`), mai una proposta o un task manuale. Funzioni pure.

use super::{ReportFields, TaskKind, TaskSpec};

/// Errore di un `kind: "plan"` arrivato da fuori (proposta, task manuale, modifica).
pub const PLAN_KIND_RESERVED: &str =
    "kind \"plan\" is reserved to the app: plan tasks cannot be proposed, created or set by hand";

impl TaskKind {
    /// Il task gira sempre in un worktree suo, anche se la modalita' di
    /// consegna non li usa (tranne il piano di una cartella senza git, in sola
    /// lettura: vedi `automation::planner::root`), e non tocca mai il checkout del repository: non
    /// occupa il repository nello scheduler e non ha setup ne' consegna.
    pub fn isolated(self) -> bool {
        self == TaskKind::Plan
    }
}

/// `kind` letto da un input esterno: `plan` non si assegna, e un task `plan`
/// non cambia tipo. `base` e' il tipo attuale del task modificato.
pub fn check_kind_input(kind: TaskKind, base: Option<TaskKind>) -> Result<(), String> {
    match (base, kind) {
        (Some(TaskKind::Plan), TaskKind::Plan) => Ok(()),
        (Some(TaskKind::Plan), _) => Err("the kind of a plan task cannot be changed".to_string()),
        (_, TaskKind::Plan) => Err(PLAN_KIND_RESERVED.to_string()),
        _ => Ok(()),
    }
}

/// Un task `plan` non scrive codice: non puo' richiedere gli E2E.
pub fn check_plan_spec(spec: &TaskSpec) -> Result<(), String> {
    if spec.kind == TaskKind::Plan && spec.e2e {
        return Err(format!("task '{}': a plan task cannot require E2E tests (e2e)", spec.id));
    }
    Ok(())
}

/// Un piano completato non consegna nulla: niente `commit` ne' `pr_url`
/// (verdetto e problemi trovati li rifiuta `check_verdict`).
pub fn check_plan_report(kind: TaskKind, completed: bool, f: &ReportFields) -> Result<(), String> {
    if kind == TaskKind::Plan && completed && (f.commit.is_some() || f.pr_url.is_some()) {
        return Err("commit and pr_url are not accepted for plan tasks: a plan delivers no code".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::check_verdict;

    fn spec(kind: TaskKind, e2e: bool) -> TaskSpec {
        TaskSpec { id: "p".into(), kind, e2e, ..TaskSpec::default() }
    }

    #[test]
    fn only_plan_tasks_are_isolated() {
        assert!(TaskKind::Plan.isolated());
        assert!(!TaskKind::Work.isolated() && !TaskKind::Review.isolated());
    }

    #[test]
    fn plan_is_never_an_input_and_a_plan_keeps_its_kind() {
        assert_eq!(check_kind_input(TaskKind::Plan, None), Err(PLAN_KIND_RESERVED.to_string()));
        assert_eq!(check_kind_input(TaskKind::Plan, Some(TaskKind::Work)), Err(PLAN_KIND_RESERVED.to_string()));
        assert_eq!(check_kind_input(TaskKind::Plan, Some(TaskKind::Review)), Err(PLAN_KIND_RESERVED.to_string()));
        assert_eq!(check_kind_input(TaskKind::Plan, Some(TaskKind::Plan)), Ok(()));
        for other in [TaskKind::Work, TaskKind::Review] {
            let e = check_kind_input(other, Some(TaskKind::Plan)).unwrap_err();
            assert!(e.contains("cannot be changed"), "{e}");
            assert_eq!(check_kind_input(other, None), Ok(()));
            assert_eq!(check_kind_input(other, Some(TaskKind::Work)), Ok(()));
        }
    }

    #[test]
    fn a_plan_cannot_require_e2e() {
        let e = check_plan_spec(&spec(TaskKind::Plan, true)).unwrap_err();
        assert!(e.contains("task 'p'") && e.contains("e2e"), "{e}");
        assert_eq!(check_plan_spec(&spec(TaskKind::Plan, false)), Ok(()));
        assert_eq!(check_plan_spec(&spec(TaskKind::Work, true)), Ok(()));
    }

    #[test]
    fn a_completed_plan_reports_no_delivery() {
        let with = |commit: Option<&str>, pr: Option<&str>| ReportFields {
            commit: commit.map(str::to_string),
            pr_url: pr.map(str::to_string),
            ..ReportFields::default()
        };
        for f in [with(Some("abc"), None), with(None, Some("https://x/pull/1"))] {
            let e = check_plan_report(TaskKind::Plan, true, &f).unwrap_err();
            assert!(e.contains("not accepted for plan tasks"), "{e}");
            assert_eq!(check_plan_report(TaskKind::Plan, false, &f), Ok(()), "failed as always");
            assert_eq!(check_plan_report(TaskKind::Work, true, &f), Ok(()));
        }
        assert_eq!(check_plan_report(TaskKind::Plan, true, &ReportFields::default()), Ok(()));
        let verdict = ReportFields { findings: vec!["x".into()], ..ReportFields::default() };
        assert!(check_verdict(TaskKind::Plan, true, &verdict).unwrap_err().contains("only accepted for review tasks"));
    }
}
