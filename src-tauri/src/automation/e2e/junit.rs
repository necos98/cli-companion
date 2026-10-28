//! Lettura dei report JUnit degli E2E.
//!
//! Il parser e' tollerante sul contenuto (Playwright, Vitest, pytest, Surefire) ma
//! rigido sulla forma: XML malformato o root inattesa sono errori. quick-xml non
//! espande mai le entita' esterne e le entita' dichiarate nel DOCTYPE vengono ignorate.

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

/// Numero massimo di test falliti riportati nel riepilogo.
pub(crate) const MAX_FAILURES: usize = 20;
/// Lunghezze massime (in caratteri) dei campi di un test fallito.
const MAX_NAME_CHARS: usize = 300;
const MAX_FILE_CHARS: usize = 300;
const MAX_MESSAGE_CHARS: usize = 500;
/// Byte di testo di un `<failure>` conservati per ricavarne il messaggio.
const MAX_FAILURE_TEXT_BYTES: usize = 8 * 1024;
/// Messaggio usato quando il report non ne contiene uno.
const DEFAULT_MESSAGE: &str = "failed";

/// Riepilogo di un report JUnit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JunitReport {
    pub tests: u32,
    /// Failure + error.
    pub failed: u32,
    pub skipped: u32,
    /// Test passati dopo un nuovo tentativo, solo se il report li espone.
    pub flaky: Option<u32>,
    /// Primi `MAX_FAILURES` test falliti, nell'ordine del report.
    pub failures: Vec<JunitFailure>,
}

impl JunitReport {
    pub fn passed(&self) -> u32 {
        self.tests.saturating_sub(self.failed).saturating_sub(self.skipped)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JunitFailure {
    pub name: String,
    pub file: Option<String>,
    pub message: String,
}

/// Contesto del `<testsuite>` aperto, per il file di ripiego dei testcase.
struct Suite {
    level: usize,
    file: Option<String>,
    name: Option<String>,
}

/// Testcase in lettura.
struct Case {
    level: usize,
    name: String,
    file: Option<String>,
    failed: bool,
    skipped: bool,
    retried: bool,
    /// Messaggio grezzo del primo `<failure>`/`<error>`.
    message: Option<String>,
}

/// Testo del primo `<failure>`/`<error>` del testcase, usato se manca `message`.
struct Collect {
    level: usize,
    attr: Option<String>,
    text: String,
}

#[derive(Default)]
struct State {
    suites: Vec<Suite>,
    case: Option<Case>,
    collect: Option<Collect>,
    tests: u32,
    failed: u32,
    skipped: u32,
    flaky: u32,
    saw_retry: bool,
    failures: Vec<JunitFailure>,
    root_is_suites: bool,
    /// Somma di failures + errors del root `<testsuites>`, se presenti.
    root_attr: Option<u32>,
    /// Somma di failures + errors dei `<testsuite>` di primo livello.
    suites_attr: u32,
}

pub(crate) fn parse(xml: &str) -> Result<JunitReport, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().check_end_names = true;
    let mut st = State::default();
    // Elementi aperti: il livello di un elemento e' il numero dei suoi antenati.
    let mut depth = 0usize;
    let mut seen_root = false;
    loop {
        let ev = reader.read_event().map_err(|e| format!("malformed JUnit XML: {e}"))?;
        match ev {
            Event::Start(e) => {
                if !seen_root {
                    seen_root = true;
                    check_root(&e, &mut st)?;
                }
                st.start(&e, depth);
                depth += 1;
            }
            Event::Empty(e) => {
                if !seen_root {
                    seen_root = true;
                    check_root(&e, &mut st)?;
                }
                st.start(&e, depth);
                st.end(depth);
                if depth == 0 {
                    break;
                }
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                st.end(depth);
                if depth == 0 {
                    break;
                }
            }
            Event::Text(t) => {
                if seen_root {
                    st.text(&t.xml10_content());
                } else if !t.xml10_content().trim().is_empty() {
                    return Err("malformed JUnit XML: text before the root element".into());
                }
            }
            Event::CData(t) => {
                if seen_root {
                    st.text(&t.xml10_content());
                }
            }
            Event::GeneralRef(r) => {
                // Solo le entita' predefinite e i riferimenti numerici; le altre si ignorano.
                if let Ok(s) = quick_xml::escape::unescape(&format!("&{};", &*r.xml10_content())) {
                    st.text(&s);
                }
            }
            Event::Eof => {
                return Err(if seen_root {
                    "malformed JUnit XML: unexpected end of file".into()
                } else {
                    "not a JUnit report: empty input".into()
                });
            }
            Event::Decl(_) | Event::Comment(_) | Event::PI(_) | Event::DocType(_) => {}
        }
    }
    let attr_failed = match st.root_attr {
        Some(n) if st.root_is_suites => n,
        _ => st.suites_attr,
    };
    Ok(JunitReport {
        tests: st.tests,
        failed: st.failed.max(attr_failed),
        skipped: st.skipped,
        flaky: st.saw_retry.then_some(st.flaky),
        failures: st.failures,
    })
}

fn check_root(e: &BytesStart, st: &mut State) -> Result<(), String> {
    match e.local_name().as_ref() {
        "testsuites" => st.root_is_suites = true,
        "testsuite" => {}
        other => {
            return Err(format!(
                "not a JUnit report: the root element is <{}>",
                clip(other, MAX_NAME_CHARS)
            ))
        }
    }
    Ok(())
}

impl State {
    fn start(&mut self, e: &BytesStart, level: usize) {
        let local = e.local_name();
        match local.as_ref() {
            "testsuites" if level == 0 => {
                self.root_attr = count_attr(e);
            }
            "testsuite" => {
                if level == 0 || (level == 1 && self.root_is_suites) {
                    self.suites_attr = self.suites_attr.saturating_add(count_attr(e).unwrap_or(0));
                }
                self.suites.push(Suite { level, file: attr(e, "file"), name: attr(e, "name") });
            }
            "testcase" if self.case.is_none() => {
                self.tests = self.tests.saturating_add(1);
                self.case = Some(Case {
                    level,
                    name: attr(e, "name").unwrap_or_default(),
                    file: attr(e, "file").or_else(|| attr(e, "classname")),
                    failed: false,
                    skipped: false,
                    retried: false,
                    message: None,
                });
            }
            "flakyFailure" | "flakyError" | "rerunFailure" | "rerunError" => {
                self.saw_retry = true;
                if let Some(c) = self.case.as_mut().filter(|c| level == c.level + 1) {
                    c.retried = true;
                }
            }
            "failure" | "error" => {
                if let Some(c) = self.case.as_mut().filter(|c| level == c.level + 1) {
                    c.failed = true;
                    if self.collect.is_none() && c.message.is_none() {
                        self.collect = Some(Collect { level, attr: attr(e, "message"), text: String::new() });
                    }
                }
            }
            "skipped" => {
                if let Some(c) = self.case.as_mut().filter(|c| level == c.level + 1) {
                    c.skipped = true;
                }
            }
            _ => {}
        }
    }

    fn end(&mut self, level: usize) {
        if self.collect.as_ref().is_some_and(|c| c.level == level) {
            if let (Some(col), Some(case)) = (self.collect.take(), self.case.as_mut()) {
                case.message = Some(col.attr.unwrap_or_else(|| first_line(&col.text)));
            }
        }
        if self.case.as_ref().is_some_and(|c| c.level == level) {
            if let Some(case) = self.case.take() {
                self.finish_case(case);
            }
        }
        if self.suites.last().is_some_and(|s| s.level == level) {
            self.suites.pop();
        }
    }

    fn finish_case(&mut self, case: Case) {
        if case.failed {
            self.failed = self.failed.saturating_add(1);
            if self.failures.len() < MAX_FAILURES {
                let suite = self.suites.last();
                let file = case
                    .file
                    .or_else(|| suite.and_then(|s| s.file.clone()))
                    .or_else(|| suite.and_then(|s| s.name.clone()))
                    .filter(|f| !f.is_empty());
                let message = one_line(case.message.as_deref().unwrap_or(""));
                self.failures.push(JunitFailure {
                    name: clip(&case.name, MAX_NAME_CHARS),
                    file: file.map(|f| clip(&f, MAX_FILE_CHARS)),
                    message: clip(if message.is_empty() { DEFAULT_MESSAGE } else { &message }, MAX_MESSAGE_CHARS),
                });
            }
        } else if case.skipped {
            self.skipped = self.skipped.saturating_add(1);
        } else if case.retried {
            self.flaky = self.flaky.saturating_add(1);
        }
    }

    fn text(&mut self, s: &str) {
        if let Some(col) = self.collect.as_mut() {
            for c in s.chars() {
                if col.text.len() >= MAX_FAILURE_TEXT_BYTES {
                    break;
                }
                col.text.push(c);
            }
        }
    }
}

/// Valore (con entita' risolte) dell'attributo `key`; `None` se assente o illeggibile.
fn attr(e: &BytesStart, key: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok())
        .map(|v| v.into_owned())
}

fn num_attr(e: &BytesStart, key: &str) -> Option<u32> {
    attr(e, key)?.trim().parse().ok()
}

/// `failures + errors` di un elemento, se ne ha almeno uno numerico.
fn count_attr(e: &BytesStart) -> Option<u32> {
    match (num_attr(e, "failures"), num_attr(e, "errors")) {
        (None, None) => None,
        (f, er) => Some(f.unwrap_or(0).saturating_add(er.unwrap_or(0))),
    }
}

/// Prima riga non vuota del testo.
fn first_line(text: &str) -> String {
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string()
}

/// Spazi consecutivi (a capo compresi) ridotti a uno solo.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYWRIGHT: &str = include_str!("fixtures/playwright.xml");
    const VITEST: &str = include_str!("fixtures/vitest.xml");
    const PYTEST: &str = include_str!("fixtures/pytest.xml");

    fn failing(n: usize) -> String {
        let cases: String = (0..n)
            .map(|i| format!(r#"<testcase name="t{i}"><failure message="m{i}"/></testcase>"#))
            .collect();
        format!("<testsuite>{cases}</testsuite>")
    }

    #[test]
    fn playwright_report() {
        let r = parse(PLAYWRIGHT).unwrap();
        assert_eq!((r.tests, r.failed, r.skipped, r.passed(), r.flaky), (4, 1, 1, 2, None));
        let f = &r.failures[0];
        assert_eq!(f.name, "fails & <escapes>");
        assert_eq!(f.file.as_deref(), Some("sample.spec.js"));
        assert!(f.message.starts_with("sample.spec.js:4:1 fails & <escapes>"));
    }

    #[test]
    fn vitest_report() {
        let r = parse(VITEST).unwrap();
        assert_eq!((r.tests, r.failed, r.skipped, r.passed()), (3, 1, 1, 1));
        let f = &r.failures[0];
        assert_eq!(f.name, "math > breaks \"quoted\"");
        assert_eq!(f.file.as_deref(), Some("src/a.test.js"));
        assert_eq!(f.message, "expected { a: 1 } to deeply equal { a: 2 }");
    }

    #[test]
    fn pytest_report() {
        let r = parse(PYTEST).unwrap();
        assert_eq!((r.tests, r.failed, r.skipped, r.passed()), (4, 2, 1, 1));
        assert_eq!(r.failures.len(), 2);
        assert!(r.failures[0].message.starts_with("AssertionError: lists differ assert [1, 2] == [1, 3]"));
        assert_eq!(r.failures[1].message, "failed on setup with \"RuntimeError: fixture exploded\"");
        assert_eq!(r.failures[1].file.as_deref(), Some("tests.test_sample"));
    }

    #[test]
    fn flaky_surefire_is_passed() {
        let xml = r#"<testsuite name="s" tests="2" failures="0">
            <testcase name="a" classname="C"><flakyFailure message="x"><stackTrace>boom</stackTrace></flakyFailure></testcase>
            <testcase name="b" classname="C"/></testsuite>"#;
        let r = parse(xml).unwrap();
        assert_eq!((r.tests, r.failed, r.passed(), r.flaky), (2, 0, 2, Some(1)));
    }

    #[test]
    fn message_from_text_or_default() {
        let xml = "<testsuite><testcase name=\"a\"><failure>\n\n  first   line \nsecond</failure></testcase>\
                   <testcase name=\"b\"><error/></testcase>\
                   <testcase name=\"c\"><failure><![CDATA[ a &gt; b ]]> &amp; c</failure></testcase></testsuite>";
        let r = parse(xml).unwrap();
        assert_eq!(r.failures[0].message, "first line");
        assert_eq!(r.failures[1].message, "failed");
        assert_eq!(r.failures[2].message, "a &gt; b & c");
    }

    #[test]
    fn file_fallbacks() {
        let xml = r#"<testsuites><testsuite name="sn" file="sf.js">
            <testcase name="a" file="cf.js" classname="cc"><failure/></testcase>
            <testcase name="b"><failure/></testcase></testsuite>
            <testsuite name="only"><testcase name="c"><failure/></testcase></testsuite></testsuites>"#;
        let r = parse(xml).unwrap();
        let files: Vec<_> = r.failures.iter().map(|f| f.file.as_deref()).collect();
        assert_eq!(files, [Some("cf.js"), Some("sf.js"), Some("only")]);
        let r = parse("<testsuite><testcase name=\"a\"><failure/></testcase></testsuite>").unwrap();
        assert_eq!(r.failures[0].file, None);
    }

    #[test]
    fn suite_level_errors_are_not_lost() {
        let r = parse(r#"<testsuites><testsuite name="s" errors="2" failures="1"/></testsuites>"#).unwrap();
        assert_eq!((r.tests, r.failed), (0, 3));
        let r = parse(r#"<testsuites errors="1"><testsuite errors="5"/></testsuites>"#).unwrap();
        assert_eq!(r.failed, 1);
        let r = parse(r#"<testsuite errors="abc" failures="2"><testcase name="a"/></testsuite>"#).unwrap();
        assert_eq!((r.tests, r.failed, r.passed()), (1, 2, 0));
        let r = parse(r#"<testsuite errors="-1" failures="x"/>"#).unwrap();
        assert_eq!(r.failed, 0);
    }

    #[test]
    fn nested_suites_counted_once() {
        let xml = r#"<testsuites><testsuite name="o" failures="1"><testsuite name="i" failures="1">
            <testcase name="a"><failure message="m"/></testcase><testcase name="b"/>
            </testsuite></testsuite></testsuites>"#;
        let r = parse(xml).unwrap();
        assert_eq!((r.tests, r.failed, r.passed()), (2, 1, 1));
    }

    #[test]
    fn invalid_roots_and_input() {
        assert!(parse("").is_err());
        assert!(parse("  \n\t ").is_err());
        assert_eq!(parse("<html><body/></html>").unwrap_err(), "not a JUnit report: the root element is <html>");
        assert!(parse("just some text").is_err());
        assert!(parse("\u{0}\u{1}\u{ff}<<<>>>&&&").is_err());
        assert!(parse("<?xml version=\"1.0\"?><!-- c --> <testsuites/>").is_ok());
    }

    #[test]
    fn malformed_xml_is_an_error() {
        for half in [PLAYWRIGHT, VITEST, PYTEST] {
            let mut cut = half.len() / 2;
            while !half.is_char_boundary(cut) {
                cut += 1;
            }
            let e = parse(&half[..cut]).unwrap_err();
            assert!(e.starts_with("malformed JUnit XML"), "{e}");
        }
        assert!(parse("<testsuite><testcase name=\"a\"></testsuite>").unwrap_err().starts_with("malformed JUnit XML"));
        assert!(parse("<testsuite>").is_err());
        assert!(parse("<testsuite><testcase").is_err());
    }

    #[test]
    fn zero_testcases_is_ok() {
        let r = parse("<testsuites/>").unwrap();
        assert_eq!((r.tests, r.failed, r.skipped, r.passed(), r.flaky), (0, 0, 0, 0, None));
        assert!(r.failures.is_empty());
    }

    #[test]
    fn doctype_entities_are_not_expanded() {
        let xml = "<?xml version=\"1.0\"?><!DOCTYPE testsuite [<!ENTITY xxe \"SECRET-CONTENT\">]>\
                   <testsuite><testcase name=\"a&xxe;\"><failure>&xxe; boom</failure></testcase>\
                   <testcase name=\"b\"><failure>&xxe;</failure></testcase></testsuite>";
        if let Ok(r) = parse(xml) {
            for f in &r.failures {
                assert!(!f.name.contains("SECRET-CONTENT") && !f.message.contains("SECRET-CONTENT"));
            }
        }
        let ext = "<!DOCTYPE t [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><testsuite><testcase name=\"a\"><failure>&x;</failure></testcase></testsuite>";
        if let Ok(r) = parse(ext) {
            assert!(!r.failures[0].message.contains("root:"));
        }
    }

    #[test]
    fn failures_are_capped() {
        let r = parse(&failing(25)).unwrap();
        assert_eq!((r.tests, r.failed), (25, 25));
        assert_eq!(r.failures.len(), MAX_FAILURES);
        assert_eq!(r.failures[0].name, "t0");
        assert_eq!(r.failures[MAX_FAILURES - 1].name, format!("t{}", MAX_FAILURES - 1));
    }

    #[test]
    fn long_and_unicode_text_is_clipped() {
        let long = "x".repeat(MAX_MESSAGE_CHARS + 50);
        let uni = "é".repeat(MAX_NAME_CHARS + 10);
        let xml = format!(r#"<testsuite><testcase name="{uni}" classname="{uni}"><failure message="{long}"/></testcase></testsuite>"#);
        let f = &parse(&xml).unwrap().failures[0];
        assert_eq!(f.message, format!("{}…", "x".repeat(MAX_MESSAGE_CHARS)));
        assert_eq!(f.name, format!("{}…", "é".repeat(MAX_NAME_CHARS)));
        assert_eq!(f.file.as_deref(), Some(f.name.as_str()));
        let exact = "y".repeat(MAX_MESSAGE_CHARS);
        let xml = format!(r#"<testsuite><testcase name="a"><failure message="{exact}"/></testcase></testsuite>"#);
        assert_eq!(parse(&xml).unwrap().failures[0].message, exact);
    }
}
