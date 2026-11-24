//! Il controllo d'accesso di `dispatch` segue la tabella di `roles`: ogni
//! coppia metodo/ruolo vietata viene rifiutata col messaggio del suo ruolo.

use super::plan::started_plan;
use super::review::{reviewer, session_of_done};
use super::*;

const RESERVED: &str = "is reserved for automated sessions";
const NOT_AVAILABLE: &str = "is not available to automated sessions";

/// Un chiamante per ogni ruolo, sulla stessa board: l'Intake, il worker del
/// task `a` (concluso, ma ancora un worker per `dispatch`), il revisore e il
/// pianificatore.
fn callers() -> (AutomationState, FakeHost, [(Role, String); 4]) {
    let (st, h, reviewer_tab) = reviewer(DeliveryMode::LocalMerge, true);
    let worker_tab = session_of_done(&st, &h);
    let (_, planner_tab) = started_plan(&st, &h);
    let callers = [
        (Role::Session, INTAKE.to_string()),
        (Role::Worker, worker_tab),
        (Role::Reviewer, reviewer_tab),
        (Role::Planner, planner_tab),
    ];
    (st, h, callers)
}

/// Messaggio atteso quando `role` chiama `method` e la tabella lo vieta.
fn rejection(method: Method, role: Role) -> String {
    let suffix = match role {
        Role::Session => RESERVED,
        Role::Worker | Role::Reviewer | Role::Planner => NOT_AVAILABLE,
    };
    format!("{} {suffix}", method.name())
}

#[test]
fn every_role_is_rejected_on_the_methods_the_table_forbids() {
    let (st, h, callers) = callers();
    for (role, tab) in &callers {
        for method in Method::ALL {
            let result = call(&st, &h, tab, method.name(), json!({}));
            if method.allows(*role) {
                // Puo' fallire per i parametri o lo stato, non per il ruolo.
                if let Err(e) = result {
                    for gate in [RESERVED, NOT_AVAILABLE, "unknown method"] {
                        assert!(!e.contains(gate), "{} / {}: {e}", role.as_str(), method.name());
                    }
                }
            } else {
                assert_eq!(result, Err(rejection(method, *role)), "{} / {}", role.as_str(), method.name());
            }
        }
    }
}

#[test]
fn the_callers_have_the_role_they_stand_for() {
    let (st, h, callers) = callers();
    for (role, tab) in &callers {
        let me = call(&st, &h, tab, "whoami", json!({})).unwrap();
        assert_eq!(me["role"], role.as_str(), "{tab}");
        // Solo la sessione normale porta il suo workspace.
        assert_eq!(me.get("workspace").is_some(), *role == Role::Session, "{tab}");
    }
}

#[test]
fn an_unknown_method_is_unknown_for_every_role() {
    let (st, h, callers) = callers();
    for (role, tab) in &callers {
        let e = call(&st, &h, tab, "get_events", json!({})).unwrap_err();
        assert_eq!(e, "unknown method: get_events", "{}", role.as_str());
    }
}

#[test]
fn params_are_checked_before_the_method() {
    let (st, h, callers) = callers();
    for (role, tab) in &callers {
        for method in ["get_events", "list_tasks", "get_task"] {
            let e = call(&st, &h, tab, method, json!([1])).unwrap_err();
            assert_eq!(e, "params must be an object", "{} / {method}", role.as_str());
        }
    }
}
