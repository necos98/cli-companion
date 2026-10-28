//! Definizione degli scenari S1, S1-it, S2, S3.

use super::verdict::Check;
use super::{Scenario, ScenarioStep, Seed};
use crate::fixture::DUPLICATE_EMAIL_ERROR;

const REGISTRATION_CHECKS: [Check; 4] = [
    Check::AccountCountDelta(1),
    Check::TestAccountExists,
    Check::UrlPathIs("/welcome"),
    Check::PageTextContains("Welcome, {name}"),
];

fn action_step(text: &'static str, checks: Vec<Check>) -> ScenarioStep {
    ScenarioStep {
        text,
        checks,
        is_report: false,
    }
}

fn registration(id: &'static str, text: &'static str) -> Scenario {
    Scenario {
        id,
        seed: Seed::Nothing,
        extras: &[],
        steps: vec![action_step(text, REGISTRATION_CHECKS.to_vec())],
    }
}

fn duplicate_registration() -> Scenario {
    Scenario {
        id: "S2",
        seed: Seed::TestAccount,
        extras: &[],
        steps: vec![ScenarioStep {
            text: "go to /register and try to create an account with the test data; \
the email is already registered, so report the error shown",
            checks: vec![
                Check::AccountCountDelta(0),
                Check::PageTextContains(DUPLICATE_EMAIL_ERROR),
                Check::DoneCalled,
                Check::DoneNoteContains("already registered"),
            ],
            is_report: true,
        }],
    }
}

fn display_name_change() -> Scenario {
    Scenario {
        id: "S3",
        seed: Seed::TestAccount,
        extras: &[("new_display_name", "Morgan Lee")],
        steps: vec![
            action_step(
                "log in with the test account",
                vec![Check::LoginRecorded, Check::UrlPathStartsWith("/app")],
            ),
            action_step(
                "open the settings page",
                vec![Check::UrlPathIs("/app/settings")],
            ),
            action_step(
                "change the display name to Morgan Lee",
                vec![Check::DisplayNameIs("{new_display_name}")],
            ),
            action_step(
                "verify that the display name is now Morgan Lee",
                vec![
                    Check::DoneSucceeded,
                    Check::DisplayNameIs("{new_display_name}"),
                    Check::PageTextContains("{new_display_name}"),
                ],
            ),
        ],
    }
}

/// Tutti gli scenari, nell'ordine di esecuzione.
pub fn all() -> Vec<Scenario> {
    vec![
        registration("S1", "go to /register and create an account"),
        registration("S1-it", "vai su /register e crea un account"),
        duplicate_registration(),
        display_name_change(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_ordered() {
        let ids: Vec<&str> = all().iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["S1", "S1-it", "S2", "S3"]);
    }

    #[test]
    fn every_step_has_checks() {
        assert!(all()
            .iter()
            .flat_map(|s| &s.steps)
            .all(|step| !step.checks.is_empty()));
    }

    #[test]
    fn only_s2_reports() {
        let reports: Vec<&str> = all()
            .iter()
            .filter(|s| s.steps.iter().any(|st| st.is_report))
            .map(|s| s.id)
            .collect();
        assert_eq!(reports, vec!["S2"]);
    }
}
