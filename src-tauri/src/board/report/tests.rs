//! `commits` e `pr_urls` del report di un task di consegna: lettura e costruzione.

use super::*;
use serde_json::json;

#[test]
fn commits_and_pr_urls_are_read_as_lists_of_tokens() {
    let f = parse_report_fields(&json!({
        "commits": [" abc123 ", "def456", ""],
        "pr_urls": ["https://github.com/o/r/pull/1", "http://host/pull/2"],
    }))
    .unwrap();
    assert_eq!(f.commits, ["abc123", "def456"], "trimmed, blanks dropped");
    assert_eq!(f.pr_urls, ["https://github.com/o/r/pull/1", "http://host/pull/2"]);
    let none = parse_report_fields(&json!({ "commits": null })).unwrap();
    assert!(none.commits.is_empty() && none.pr_urls.is_empty());
    assert!(parse_report_fields(&json!({})).unwrap().commits.is_empty());
}

#[test]
fn invalid_commits_and_pr_urls_are_refused() {
    let err = |v: serde_json::Value| parse_report_fields(&v).unwrap_err();
    assert_eq!(err(json!({ "commits": "abc" })), "commits must be an array of strings");
    assert_eq!(err(json!({ "pr_urls": { "a": 1 } })), "pr_urls must be an array of strings");
    assert_eq!(err(json!({ "commits": [1] })), "commits must be a string");
    assert_eq!(err(json!({ "commits": ["a b"] })), "invalid commits (a hash or ref, no spaces)");
    assert_eq!(err(json!({ "commits": ["a\nb"] })), "invalid commits (a hash or ref, no spaces)");
    assert_eq!(err(json!({ "commits": ["x".repeat(COMMIT_MAX + 1)] })), "invalid commits (a hash or ref, no spaces)");
    assert_eq!(err(json!({ "pr_urls": ["ftp://x/1"] })), "invalid pr_urls (an http(s) link)");
    assert_eq!(err(json!({ "pr_urls": ["https://x/1 2"] })), "invalid pr_urls (a link, no spaces)");
    let many: Vec<String> = (0..=DELIVERED_MAX).map(|i| format!("c{i}")).collect();
    assert_eq!(err(json!({ "commits": many })), format!("commits has too many entries (max {DELIVERED_MAX})"));
    // Il singolo `pr_url` resta com'era.
    assert_eq!(err(json!({ "pr_url": "ftp://x" })), "invalid pr_url (an http(s) link)");
    let ok: Vec<String> = (0..DELIVERED_MAX).map(|i| format!("c{i}")).collect();
    assert_eq!(parse_report_fields(&json!({ "commits": ok })).unwrap().commits.len(), DELIVERED_MAX);
}

#[test]
fn the_report_carries_them_and_omits_the_empty_lists() {
    let f =
        ReportFields { commits: vec!["abc".into()], pr_urls: vec!["https://x/1".into()], ..ReportFields::default() };
    let report = build_report("completed", "done", f, None);
    assert_eq!(
        (report.commits.as_slice(), report.pr_urls.as_slice()),
        (&["abc".to_string()][..], &["https://x/1".to_string()][..])
    );
    let v = serde_json::to_value(&report).unwrap();
    assert_eq!((v["commits"].clone(), v["pr_urls"].clone()), (json!(["abc"]), json!(["https://x/1"])));
    let plain = serde_json::to_value(build_report("completed", "done", ReportFields::default(), None)).unwrap();
    assert!(plain.get("commits").is_none() && plain.get("pr_urls").is_none());
    // Un report scritto prima dei campi nuovi si legge.
    let old: TaskReport = serde_json::from_value(json!({ "status": "completed", "summary": "s" })).unwrap();
    assert!(old.commits.is_empty() && old.pr_urls.is_empty());
}
