//! Verdetto deterministico di un passo, calcolato dallo stato del fixture e dalla pagina.

use super::test_data::{TestData, KEY_EMAIL, KEY_NAME};
use super::ScenarioStep;
use crate::agent::DoneReport;
use crate::fixture::state::StateSnapshot;

/// Controllo su un passo. I testi `&str` sono template (`{chiave}` dai dati di test).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    /// Variazione del numero di account rispetto a prima del passo.
    AccountCountDelta(isize),
    /// Esiste un account con email e nome dei dati di test.
    TestAccountExists,
    UrlPathIs(&'static str),
    UrlPathStartsWith(&'static str),
    PageTextContains(&'static str),
    /// Un nuovo login riuscito per l'email di test.
    LoginRecorded,
    /// L'account di test ha questo nome visualizzato.
    DisplayNameIs(&'static str),
    DoneCalled,
    DoneSucceeded,
    DoneNoteContains(&'static str),
}

pub struct CheckContext<'a> {
    pub before: &'a StateSnapshot,
    pub after: &'a StateSnapshot,
    pub url: &'a str,
    pub page_text: &'a str,
    pub done: Option<&'a DoneReport>,
    pub data: &'a TestData,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub passed: bool,
    pub failed_checks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judgement {
    pub verdict: Verdict,
    /// `done(success=true)` ma verdetto negativo.
    pub false_success: bool,
    /// `done(success=false)` ma verdetto positivo.
    pub false_failure: bool,
}

impl Check {
    pub fn describe(&self) -> String {
        match self {
            Check::AccountCountDelta(n) => format!("account_count_delta({n})"),
            Check::TestAccountExists => "test_account_exists".to_string(),
            Check::UrlPathIs(p) => format!("url_path_is({p})"),
            Check::UrlPathStartsWith(p) => format!("url_path_starts_with({p})"),
            Check::PageTextContains(t) => format!("page_text_contains({t})"),
            Check::LoginRecorded => "login_recorded".to_string(),
            Check::DisplayNameIs(t) => format!("display_name_is({t})"),
            Check::DoneCalled => "done_called".to_string(),
            Check::DoneSucceeded => "done_succeeded".to_string(),
            Check::DoneNoteContains(t) => format!("done_note_contains({t})"),
        }
    }

    pub fn passes(&self, ctx: &CheckContext) -> bool {
        let data = ctx.data;
        match self {
            Check::AccountCountDelta(delta) => {
                let now = ctx.after.accounts.len() as isize;
                now - ctx.before.accounts.len() as isize == *delta
            }
            Check::TestAccountExists => ctx
                .after
                .accounts
                .iter()
                .any(|a| a.email == data.get(KEY_EMAIL) && a.name == data.get(KEY_NAME)),
            Check::UrlPathIs(path) => url_path(ctx.url) == *path,
            Check::UrlPathStartsWith(prefix) => url_path(ctx.url).starts_with(prefix),
            Check::PageTextContains(template) => {
                collapse(ctx.page_text).contains(&collapse(&data.render(template)))
            }
            Check::LoginRecorded => {
                logins_of(ctx.after, data.get(KEY_EMAIL))
                    > logins_of(ctx.before, data.get(KEY_EMAIL))
            }
            Check::DisplayNameIs(template) => {
                let expected = data.render(template);
                ctx.after
                    .accounts
                    .iter()
                    .any(|a| a.email == data.get(KEY_EMAIL) && a.display_name == expected)
            }
            Check::DoneCalled => ctx.done.is_some(),
            Check::DoneSucceeded => ctx.done.is_some_and(|d| d.success),
            Check::DoneNoteContains(text) => ctx
                .done
                .is_some_and(|d| d.note.to_lowercase().contains(&text.to_lowercase())),
        }
    }
}

/// Valuta tutti i controlli del passo (nessun corto circuito: servono i nomi dei falliti).
pub fn judge(step: &ScenarioStep, ctx: &CheckContext) -> Judgement {
    let failed_checks: Vec<String> = step
        .checks
        .iter()
        .filter(|check| !check.passes(ctx))
        .map(Check::describe)
        .collect();
    let passed = failed_checks.is_empty();
    let claimed = if step.is_report {
        None
    } else {
        ctx.done.map(|d| d.success)
    };
    Judgement {
        verdict: Verdict {
            passed,
            failed_checks,
        },
        false_success: claimed == Some(true) && !passed,
        false_failure: claimed == Some(false) && passed,
    }
}

/// Path (senza query/fragment) di un URL assoluto o relativo.
pub fn url_path(url: &str) -> &str {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = after_scheme.find('/').map_or("/", |i| &after_scheme[i..]);
    path.split(['?', '#']).next().unwrap_or("/")
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn logins_of(state: &StateSnapshot, email: &str) -> usize {
    state.logins.iter().filter(|e| e.as_str() == email).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::state::AccountView;

    fn state(accounts: &[(&str, &str, &str)], logins: &[&str]) -> StateSnapshot {
        StateSnapshot {
            accounts: accounts
                .iter()
                .map(|(name, email, display)| AccountView {
                    name: name.to_string(),
                    email: email.to_string(),
                    display_name: display.to_string(),
                })
                .collect(),
            logins: logins.iter().map(|s| s.to_string()).collect(),
            active_sessions: 0,
        }
    }

    fn data() -> TestData {
        TestData::generate("base", "S3", 1, &[("new_display_name", "Morgan Lee")])
    }

    fn account<'a>(data: &'a TestData, display: &'a str) -> (&'a str, &'a str, &'a str) {
        (data.get(KEY_NAME), data.get(KEY_EMAIL), display)
    }

    struct Fixture {
        before: StateSnapshot,
        after: StateSnapshot,
        data: TestData,
    }

    impl Fixture {
        fn ctx<'a>(
            &'a self,
            url: &'a str,
            text: &'a str,
            done: Option<&'a DoneReport>,
        ) -> CheckContext<'a> {
            CheckContext {
                before: &self.before,
                after: &self.after,
                url,
                page_text: text,
                done,
                data: &self.data,
            }
        }
    }

    fn registered() -> Fixture {
        let data = data();
        let after = state(&[account(&data, data.get(KEY_NAME))], &[]);
        Fixture {
            before: state(&[], &[]),
            after,
            data,
        }
    }

    fn done(success: bool, note: &str) -> DoneReport {
        DoneReport {
            success,
            note: note.to_string(),
        }
    }

    fn step(checks: Vec<Check>, is_report: bool) -> ScenarioStep {
        ScenarioStep {
            text: "t",
            checks,
            is_report,
        }
    }

    #[test]
    fn registration_checks_pass_on_a_new_account() {
        let f = registered();
        let text = format!("Welcome, {}", f.data.get(KEY_NAME));
        let ctx = f.ctx("http://127.0.0.1:8790/welcome", &text, None);
        for check in [
            Check::AccountCountDelta(1),
            Check::TestAccountExists,
            Check::UrlPathIs("/welcome"),
            Check::PageTextContains("Welcome, {name}"),
        ] {
            assert!(check.passes(&ctx), "{}", check.describe());
        }
        assert!(!Check::AccountCountDelta(0).passes(&ctx));
    }

    #[test]
    fn url_checks_ignore_query_and_origin() {
        assert_eq!(url_path("http://h:1/app/settings?x=1#y"), "/app/settings");
        assert_eq!(url_path("http://h:1"), "/");
        let f = registered();
        let ctx = f.ctx("http://h:1/app/settings?x=1", "", None);
        assert!(Check::UrlPathStartsWith("/app").passes(&ctx));
        assert!(!Check::UrlPathIs("/app").passes(&ctx));
    }

    #[test]
    fn login_is_recorded_only_when_it_grows() {
        let data = data();
        let email = data.get(KEY_EMAIL).to_string();
        let before = state(&[], &[&email]);
        let same = Fixture {
            before: before.clone(),
            after: before.clone(),
            data: data.clone(),
        };
        assert!(!Check::LoginRecorded.passes(&same.ctx("", "", None)));
        let grown = Fixture {
            before: before.clone(),
            after: state(&[], &[&email, &email]),
            data,
        };
        assert!(Check::LoginRecorded.passes(&grown.ctx("", "", None)));
    }

    #[test]
    fn display_name_uses_templates() {
        let data = data();
        let after = state(&[account(&data, "Morgan Lee")], &[]);
        let f = Fixture {
            before: after.clone(),
            after,
            data,
        };
        let ctx = f.ctx("", "Display name: Morgan Lee", None);
        assert!(Check::DisplayNameIs("{new_display_name}").passes(&ctx));
        assert!(Check::PageTextContains("{new_display_name}").passes(&ctx));
        assert!(!Check::DisplayNameIs("Other").passes(&ctx));
    }

    #[test]
    fn done_checks_look_at_the_report() {
        let f = registered();
        let ok = done(true, "Error: This email is ALREADY REGISTERED.");
        let ctx = f.ctx("", "", Some(&ok));
        assert!(Check::DoneCalled.passes(&ctx));
        assert!(Check::DoneSucceeded.passes(&ctx));
        assert!(Check::DoneNoteContains("already registered").passes(&ctx));
        let bad = done(false, "nothing");
        let ctx = f.ctx("", "", Some(&bad));
        assert!(!Check::DoneSucceeded.passes(&ctx));
        assert!(!Check::DoneNoteContains("already registered").passes(&ctx));
        assert!(!Check::DoneCalled.passes(&f.ctx("", "", None)));
    }

    #[test]
    fn false_success_and_false_failure_are_detected() {
        let f = registered();
        let failing = step(vec![Check::UrlPathIs("/nope")], false);
        let passing = step(vec![Check::AccountCountDelta(1)], false);
        let claim_ok = done(true, "");
        let claim_ko = done(false, "");

        let j = judge(&failing, &f.ctx("http://h/x", "", Some(&claim_ok)));
        assert!(!j.verdict.passed && j.false_success && !j.false_failure);
        assert_eq!(j.verdict.failed_checks, vec!["url_path_is(/nope)"]);

        let j = judge(&passing, &f.ctx("http://h/x", "", Some(&claim_ko)));
        assert!(j.verdict.passed && j.false_failure && !j.false_success);

        let j = judge(&passing, &f.ctx("http://h/x", "", None));
        assert!(j.verdict.passed && !j.false_failure && !j.false_success);
    }

    #[test]
    fn report_steps_do_not_count_the_success_flag() {
        let f = registered();
        let report = step(vec![Check::UrlPathIs("/nope")], true);
        let claim = done(true, "");
        let j = judge(&report, &f.ctx("http://h/x", "", Some(&claim)));
        assert!(!j.verdict.passed && !j.false_success);
    }
}
