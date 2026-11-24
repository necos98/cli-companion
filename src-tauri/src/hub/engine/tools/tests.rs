use super::*;
use crate::board::ANSWER_MAX;
use crate::board::{TaskKind, TaskStatus};
use crate::hub::engine::board_query::{PAGE_DEFAULT, PAGE_MAX, QUERY_MAX};
use crate::hub::engine::relay::QuestionSelector;
use crate::hub::engine::task_page::TaskPage;
use crate::hub::engine::thread_page::{ThreadPage, MESSAGE_ID_MAX};
use crate::hub::engine::tools::relay_answer::QUESTION_ID_MAX;
use crate::hub::engine::tools::task_details::TASK_ID_MAX;
use crate::hub::engine::workspace_names::REFERENCE_MAX;
use crate::hub::settings::SecretaryChannels;
use crate::hub::text::BODY_MAX;

fn secretary(tools: &[SecretaryTool]) -> Secretary {
    Secretary {
        name: "mario".into(),
        image: None,
        system_prompt: "Report.".into(),
        tools: tools.to_vec(),
        channels: SecretaryChannels::All,
        preset: None,
    }
}

fn call(name: &str, arguments: &str) -> ToolCall {
    ToolCall { id: "c".into(), name: name.into(), arguments: arguments.into() }
}

#[test]
fn only_the_allowed_tools_are_offered() {
    let s = secretary(&[SecretaryTool::PostMessage, SecretaryTool::BoardStatus]);
    let names: Vec<&str> = specs(&s).iter().map(|t| t.name).collect();
    assert_eq!(names, ["board_status", "post_message"]);
    assert!(specs(&secretary(&[])).is_empty());
}

#[test]
fn every_schema_is_a_closed_object_with_an_english_description() {
    for tool in SecretaryTool::ALL {
        let s = spec(tool);
        assert_eq!(s.name, tool.code());
        assert_eq!(s.parameters["type"], "object");
        assert_eq!(s.parameters["additionalProperties"], false);
        assert!(s.description.is_ascii() && !s.description.is_empty());
    }
}

#[test]
fn schema_properties_match_the_argument_structs() {
    let all = secretary(&SecretaryTool::ALL);
    let samples = [
        (
            "board_status",
            json!({"workspace": "ws-1", "status": ["failed"], "attention": true, "kind": "work", "query": "login", "updated_since": "2026-11-01", "updated_before": "2026-11-05", "offset": 0, "limit": 5}),
        ),
        ("board_overview", json!({})),
        ("task_details", json!({"task_id": "t-1", "workspace": "ws-1", "field": "task", "offset": 0})),
        ("post_message", json!({"body": "x"})),
        ("open_plan_task", json!({"goal": "Add a dark theme", "bot": "general"})),
        ("decide_proposal", json!({"accept": true})),
        ("relay_answer", json!({"body": "Use the second option.", "question_id": "q-1-1", "task_id": "t-1"})),
        (
            "task_action",
            json!({"task_id": "t-1", "action": "move_before", "before_task_id": "t-2", "workspace": "ws-1"}),
        ),
        ("board_control", json!({"paused": true, "max_parallel": 2, "workspace": "ws-1"})),
        ("request_task_action", json!({"task_id": "t-1", "action": "delete_cascade", "reason": "Obsolete"})),
    ];
    for (name, sample) in samples {
        let tool = SecretaryTool::ALL.into_iter().find(|t| t.code() == name).unwrap();
        let props = spec(tool).parameters["properties"].as_object().unwrap().keys().cloned().collect::<Vec<_>>();
        let keys = sample.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
        assert_eq!(sorted(props), sorted(keys), "{name}");
        assert!(parse(&call(name, &sample.to_string()), &all).is_ok(), "{name}");
    }
}

#[test]
fn a_tool_outside_the_list_is_not_allowed() {
    let s = secretary(&[SecretaryTool::ThreadRead]);
    assert_eq!(parse(&call("board_status", "{}"), &s), Err(ToolRejection::NotAllowed("board_status".into())));
    assert_eq!(parse(&call("rm\n-rf", "{}"), &s), Err(ToolRejection::NotAllowed("rm -rf".into())));
    let long = "x".repeat(200);
    let Err(ToolRejection::NotAllowed(name)) = parse(&call(&long, "{}"), &s) else { panic!() };
    assert!(name.chars().count() <= UNKNOWN_NAME_KEPT + 1);
}

#[test]
fn arguments_are_validated_against_the_schema() {
    let s = secretary(&SecretaryTool::ALL);
    assert_eq!(parse(&call("thread_read", ""), &s), Ok(Invocation::ThreadRead { page: ThreadPage::Latest }));
    assert_eq!(
        parse(&call("board_status", "{}"), &s),
        Ok(Invocation::BoardStatus { workspace: None, query: BoardQuery::default() })
    );
    assert_eq!(
        parse(&call("board_status", r#"{"workspace":" ws-2 "}"#), &s),
        Ok(Invocation::BoardStatus { workspace: Some("ws-2".into()), query: BoardQuery::default() })
    );
    assert_eq!(
        parse(&call("board_status", r#"{"workspace":" Sito Web "}"#), &s),
        Ok(Invocation::BoardStatus { workspace: Some("Sito Web".into()), query: BoardQuery::default() })
    );
    assert_eq!(
        parse(&call("post_message", r#"{"body":"  Controllo.  "}"#), &s),
        Ok(Invocation::PostMessage { body: "Controllo.".into() })
    );
    let too_long = format!(r#"{{"workspace":"{}"}}"#, "x".repeat(REFERENCE_MAX + 1));
    for (name, args) in [
        ("thread_read", r#"{"limit":3}"#),
        ("thread_read", "[]"),
        ("thread_read", "{"),
        ("board_status", too_long.as_str()),
        ("board_status", r#"{"workspace":3}"#),
        ("post_message", "{}"),
        ("post_message", r#"{"body":"   "}"#),
        ("post_message", r#"{"body":"a","extra":1}"#),
    ] {
        let err = parse(&call(name, args), &s).unwrap_err();
        assert!(matches!(err, ToolRejection::InvalidArguments { .. }), "{name} {args}");
        assert!(err.for_model().contains(name));
    }
    let long = "a".repeat(BODY_MAX + 1);
    assert_eq!(
        parse(&call("post_message", &json!({ "body": long }).to_string()), &s),
        Ok(Invocation::PostMessage { body: long }),
        "a text longer than one message is accepted: it is posted in parts"
    );
    let too_long = json!({ "body": "a".repeat(LONG_BODY_MAX + 1) }).to_string();
    let err = parse(&call("post_message", &too_long), &s).unwrap_err();
    assert!(err.for_model().contains(&LONG_BODY_MAX.to_string()), "{}", err.for_model());
}

#[test]
fn the_planning_tools_validate_their_arguments() {
    let s = secretary(&SecretaryTool::ALL);
    assert_eq!(
        parse(&call("open_plan_task", r#"{"goal":"  Add X to the app\u0007 ","bot":" intake "}"#), &s),
        Ok(Invocation::OpenPlanTask { goal: "Add X to the app".into(), bot: "intake".into() })
    );
    assert_eq!(
        parse(&call("decide_proposal", r#"{"accept":false}"#), &s),
        Ok(Invocation::DecideProposal { accept: false })
    );
    for (name, args) in [
        ("open_plan_task", "{}"),
        ("open_plan_task", r#"{"goal":"   ","bot":"general"}"#),
        ("open_plan_task", r#"{"goal":"Add X"}"#),
        ("open_plan_task", r#"{"goal":"Add X","bot":"   "}"#),
        ("open_plan_task", r#"{"goal":"Add X","bot":null}"#),
        ("open_plan_task", r#"{"goal":"x","bot":"general","workspace":"ws-1"}"#),
        ("decide_proposal", "{}"),
        ("decide_proposal", r#"{"accept":"yes"}"#),
        ("decide_proposal", r#"{"accept":true,"proposal_id":"p-1"}"#),
    ] {
        let err = parse(&call(name, args), &s).unwrap_err();
        assert!(matches!(err, ToolRejection::InvalidArguments { .. }), "{name} {args}");
    }
    let long = json!({ "goal": "é".repeat(GOAL_MAX + 1), "bot": "general" }).to_string();
    assert!(parse(&call("open_plan_task", &long), &s).is_err());
    assert!(parse(&call("open_plan_task", &json!({ "goal": "é".repeat(GOAL_MAX), "bot": "general" }).to_string()), &s)
        .is_ok());
    let long_bot = json!({ "goal": "x", "bot": "b".repeat(BOT_ID_MAX + 1) }).to_string();
    assert!(parse(&call("open_plan_task", &long_bot), &s).is_err());
    // Un segretario senza i tool nuovi (salvato prima) non li ha.
    let old = secretary(&[SecretaryTool::BoardStatus]);
    assert_eq!(
        parse(&call("decide_proposal", r#"{"accept":true}"#), &old),
        Err(ToolRejection::NotAllowed("decide_proposal".into()))
    );
}

#[test]
fn relay_answer_takes_a_body_within_the_answer_limit_and_optional_ids() {
    let s = secretary(&SecretaryTool::ALL);
    assert_eq!(
        parse(&call("relay_answer", r#"{"body":"  Usa Postgres.  "}"#), &s),
        Ok(Invocation::RelayAnswer { body: "Usa Postgres.".into(), selector: QuestionSelector::default() })
    );
    assert_eq!(
        parse(&call("relay_answer", r#"{"body":"x","question_id":" q-1-1 ","task_id":"t-2"}"#), &s),
        Ok(Invocation::RelayAnswer {
            body: "x".into(),
            selector: QuestionSelector { question_id: Some("q-1-1".into()), task_id: Some("t-2".into()) }
        })
    );
    assert_eq!(
        parse(&call("relay_answer", r#"{"body":"x","question_id":"  ","task_id":""}"#), &s),
        Ok(Invocation::RelayAnswer { body: "x".into(), selector: QuestionSelector::default() }),
        "un id vuoto vale assente"
    );
    let at_limit = json!({ "body": "é".repeat(ANSWER_MAX) }).to_string();
    assert!(parse(&call("relay_answer", &at_limit), &s).is_ok());
    let too_long = json!({ "body": "é".repeat(ANSWER_MAX + 1) }).to_string();
    let long_question = json!({ "body": "x", "question_id": "q".repeat(QUESTION_ID_MAX + 1) }).to_string();
    let long_task = json!({ "body": "x", "task_id": "t".repeat(TASK_ID_MAX + 1) }).to_string();
    let bad = [
        "{}",
        r#"{"body":"   "}"#,
        r#"{"body":"x","question_id":7}"#,
        r#"{"body":"x","other":"q-1-1"}"#,
        too_long.as_str(),
        long_question.as_str(),
        long_task.as_str(),
    ];
    for args in bad {
        let err = parse(&call("relay_answer", args), &s).unwrap_err();
        assert!(matches!(err, ToolRejection::InvalidArguments { tool: SecretaryTool::RelayAnswer, .. }), "{args}");
    }
    let old = secretary(&[SecretaryTool::DecideProposal]);
    assert_eq!(
        parse(&call("relay_answer", r#"{"body":"x"}"#), &old),
        Err(ToolRejection::NotAllowed("relay_answer".into()))
    );
}

#[test]
fn the_board_is_the_channel_one_or_one_of_the_secretary_channels() {
    let ws = |id: &str| Channel::Workspace(id.into());
    let mut s = secretary(&[SecretaryTool::BoardStatus]);
    assert_eq!(board_workspace(None, &ws("ws-1"), &s), Ok("ws-1".into()));
    assert_eq!(board_workspace(None, &Channel::General, &s), Err(BoardAccessError::NoChannelBoard));
    assert_eq!(board_workspace(Some("ws-2".into()), &Channel::General, &s), Ok("ws-2".into()));
    s.channels = SecretaryChannels::Only { channels: vec![Channel::General, ws("ws-1")] };
    assert_eq!(board_workspace(Some("ws-1".into()), &Channel::General, &s), Ok("ws-1".into()));
    assert_eq!(board_workspace(Some("ws-2".into()), &ws("ws-1"), &s), Err(BoardAccessError::NotAllowed("ws-2".into())));
}

fn board_args(args: &str) -> Result<(Option<String>, BoardQuery), ToolRejection> {
    match parse(&call("board_status", args), &secretary(&SecretaryTool::ALL))? {
        Invocation::BoardStatus { workspace, query } => Ok((workspace, query)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn board_status_without_arguments_asks_for_the_first_page_of_everything() {
    let (workspace, query) = board_args("{}").unwrap();
    assert_eq!((workspace, query.offset, query.limit), (None, 0, PAGE_DEFAULT));
    assert_eq!(query, BoardQuery::default());
    assert_eq!(PAGE_DEFAULT, 20);
}

#[test]
fn board_status_accepts_every_filter_and_the_page() {
    let (workspace, query) = board_args(
        r#"{"workspace":" Sito Web ","status":["failed","running"],"attention":true,"kind":"review",
            "query":"  Login  ","offset":40,"limit":5}"#,
    )
    .unwrap();
    assert_eq!(workspace.as_deref(), Some("Sito Web"));
    assert_eq!(
        query,
        BoardQuery {
            statuses: vec![TaskStatus::Failed, TaskStatus::Running],
            attention: true,
            kind: Some(TaskKind::Review),
            text: Some("Login".into()),
            updated_since: None,
            updated_before: None,
            offset: 40,
            limit: 5,
        }
    );
}

#[test]
fn board_status_reads_an_empty_query_as_no_search() {
    assert_eq!(board_args(r#"{"query":"   "}"#).unwrap().1.text, None);
    assert_eq!(board_args(r#"{"query":""}"#).unwrap().1.text, None);
    assert_eq!(board_args(r#"{"status":[]}"#).unwrap().1.statuses, Vec::<TaskStatus>::new());
}

#[test]
fn board_status_limits_are_inclusive() {
    assert_eq!(board_args(&format!(r#"{{"limit":{PAGE_MAX}}}"#)).unwrap().1.limit, PAGE_MAX);
    assert_eq!(board_args(r#"{"limit":1}"#).unwrap().1.limit, 1);
    let longest = json!({ "query": "è".repeat(QUERY_MAX) }).to_string();
    assert_eq!(board_args(&longest).unwrap().1.text.map(|t| t.chars().count()), Some(QUERY_MAX));
}

#[test]
fn board_status_rejects_invalid_arguments_with_a_clear_reason() {
    let too_long = json!({ "query": "x".repeat(QUERY_MAX + 1) }).to_string();
    let too_many = format!(r#"{{"limit":{}}}"#, PAGE_MAX + 1);
    let cases = [
        (r#"{"status":["queued","archived"]}"#, "archived"),
        (r#"{"status":["done","done"]}"#, "repeat"),
        (r#"{"status":"done"}"#, "schema"),
        (r#"{"kind":"epic"}"#, "epic"),
        (r#"{"attention":"yes"}"#, "schema"),
        (r#"{"limit":0}"#, "`limit` must be between 1 and 40"),
        (too_many.as_str(), "`limit` must be between 1 and 40"),
        (r#"{"limit":-1}"#, "schema"),
        (r#"{"limit":2.5}"#, "schema"),
        (r#"{"offset":-1}"#, "schema"),
        (r#"{"page":2}"#, "page"),
        (too_long.as_str(), "`query` must be at most"),
        (r#"{"query":5}"#, "schema"),
    ];
    for (args, reason) in cases {
        let Err(ToolRejection::InvalidArguments { tool, reason: got }) = board_args(args) else {
            panic!("{args} should be rejected");
        };
        assert_eq!(tool, SecretaryTool::BoardStatus, "{args}");
        assert!(got.contains(reason), "{args}: {got}");
    }
}

#[test]
fn the_board_status_schema_describes_every_property_and_its_bounds() {
    let spec = spec(SecretaryTool::BoardStatus);
    assert!(spec.description.contains("newest first") && spec.description.contains("next_offset"));
    let props = &spec.parameters["properties"];
    for name in ["workspace", "status", "attention", "kind", "query", "offset", "limit"] {
        assert!(props[name]["description"].as_str().is_some_and(|d| !d.is_empty()), "{name}");
    }
    assert_eq!(props["status"]["items"]["enum"], json!(["queued", "running", "done", "failed"]));
    assert_eq!(props["kind"]["enum"], json!(["work", "review", "plan"]));
    assert_eq!(
        (&props["offset"]["minimum"], &props["limit"]["minimum"], &props["limit"]["maximum"]),
        (&json!(0), &json!(1), &json!(PAGE_MAX))
    );
    assert_eq!(props["query"]["maxLength"], QUERY_MAX);
    assert_eq!(spec.parameters["additionalProperties"], false);
}

fn sorted(mut keys: Vec<String>) -> Vec<String> {
    keys.sort();
    keys
}

fn instant(text: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(text).unwrap().timestamp_millis()
}

#[test]
fn board_status_reads_the_update_window_as_utc_milliseconds() {
    let query =
        board_args(r#"{"updated_since":" 2026-11-01 ","updated_before":"2026-11-05T10:30:00+01:00"}"#).unwrap().1;
    assert_eq!(query.updated_since, Some(instant("2026-11-01T00:00:00Z")));
    assert_eq!(query.updated_before, Some(instant("2026-11-05T09:30:00Z")));
    let open = board_args(r#"{"updated_before":"2026-11-05"}"#).unwrap().1;
    assert_eq!((open.updated_since, open.updated_before.is_some()), (None, true));
}

#[test]
fn board_status_rejects_an_invalid_or_inverted_window() {
    let cases = [
        (r#"{"updated_since":"yesterday"}"#, "`updated_since` must be an ISO 8601"),
        (r#"{"updated_before":"2026-13-01"}"#, "`updated_before` must be an ISO 8601"),
        (r#"{"updated_since":""}"#, "`updated_since` must be an ISO 8601"),
        (r#"{"updated_since":5}"#, "schema"),
        (r#"{"updated_since":"2026-11-05","updated_before":"2026-11-05"}"#, "earlier than"),
        (r#"{"updated_since":"2026-11-06","updated_before":"2026-11-05"}"#, "earlier than"),
    ];
    for (args, reason) in cases {
        let Err(ToolRejection::InvalidArguments { tool, reason: got }) = board_args(args) else {
            panic!("{args} should be rejected");
        };
        assert_eq!(tool, SecretaryTool::BoardStatus, "{args}");
        assert!(got.contains(reason), "{args}: {got}");
    }
}

fn thread_page(args: &str) -> Result<ThreadPage, ToolRejection> {
    match parse(&call("thread_read", args), &secretary(&SecretaryTool::ALL))? {
        Invocation::ThreadRead { page } => Ok(page),
        other => panic!("{other:?}"),
    }
}

#[test]
fn thread_read_chooses_the_page_by_message_id_or_offset() {
    assert_eq!(thread_page("{}"), Ok(ThreadPage::Latest));
    assert_eq!(thread_page(r#"{"before":" m12 "}"#), Ok(ThreadPage::Before("m12".into())));
    assert_eq!(thread_page(r#"{"offset":40}"#), Ok(ThreadPage::Offset(40)));
    assert_eq!(thread_page(r#"{"offset":0}"#), Ok(ThreadPage::Offset(0)));
    let schema = spec(SecretaryTool::ThreadRead).parameters;
    assert_eq!(
        sorted(schema["properties"].as_object().unwrap().keys().cloned().collect()),
        ["before", "message_id", "offset"]
    );
    assert_eq!(schema["properties"]["offset"]["minimum"], 0);
}

#[test]
fn thread_read_reads_one_message_in_characters_from_its_id() {
    let body = |id: &str, offset| ThreadPage::Body { message_id: id.into(), offset };
    assert_eq!(thread_page(r#"{"message_id":" m12 "}"#), Ok(body("m12", 0)));
    assert_eq!(thread_page(r#"{"message_id":"m12","offset":1000}"#), Ok(body("m12", 1000)));
}

#[test]
fn thread_read_rejects_a_bad_page_with_a_clear_reason() {
    let long = json!({ "before": "m".repeat(MESSAGE_ID_MAX + 1) }).to_string();
    let cases = [
        (r#"{"before":"m1","offset":3}"#, "only one of"),
        (r#"{"before":"m1","message_id":"m2"}"#, "only one of `before` and `message_id`"),
        (r#"{"before":"m1","message_id":"m2","offset":3}"#, "only one of `before` and `message_id`"),
        (r#"{"message_id":"   "}"#, "`message_id` must be a message id"),
        (r#"{"message_id":"m1","offset":-1}"#, "schema"),
        (r#"{"before":"   "}"#, "`before` must be a message id"),
        (long.as_str(), "`before` must be a message id"),
        (r#"{"offset":-1}"#, "schema"),
        (r#"{"offset":"3"}"#, "schema"),
        (r#"{"after":"m1"}"#, "after"),
    ];
    for (args, reason) in cases {
        let Err(ToolRejection::InvalidArguments { tool, reason: got }) = thread_page(args) else {
            panic!("{args} should be rejected");
        };
        assert_eq!(tool, SecretaryTool::ThreadRead, "{args}");
        assert!(got.contains(reason), "{args}: {got}");
    }
}

#[test]
fn task_details_takes_a_task_id_and_an_optional_workspace() {
    let s = secretary(&SecretaryTool::ALL);
    assert_eq!(
        parse(&call("task_details", r#"{"task_id":" t-7 "}"#), &s),
        Ok(Invocation::TaskDetails { workspace: None, task_id: "t-7".into(), page: None })
    );
    assert_eq!(
        parse(&call("task_details", r#"{"task_id":"t-7","workspace":" Sito Web "}"#), &s),
        Ok(Invocation::TaskDetails { workspace: Some("Sito Web".into()), task_id: "t-7".into(), page: None })
    );
    let long_id = json!({ "task_id": "t".repeat(TASK_ID_MAX + 1) }).to_string();
    let long_workspace = json!({ "task_id": "t-1", "workspace": "w".repeat(REFERENCE_MAX + 1) }).to_string();
    for args in
        ["{}", r#"{"task_id":"  "}"#, r#"{"task_id":4}"#, r#"{"task_id":"t-1","id":"t-1"}"#, &long_id, &long_workspace]
    {
        let err = parse(&call("task_details", args), &s).unwrap_err();
        assert!(matches!(err, ToolRejection::InvalidArguments { tool: SecretaryTool::TaskDetails, .. }), "{args}");
    }
}

#[test]
fn task_details_reads_one_field_from_an_offset() {
    let s = secretary(&SecretaryTool::ALL);
    let page = |args: &str| match parse(&call("task_details", args), &s) {
        Ok(Invocation::TaskDetails { page, .. }) => Ok(page),
        Ok(other) => panic!("{other:?}"),
        Err(ToolRejection::InvalidArguments { tool: SecretaryTool::TaskDetails, reason }) => Err(reason),
        Err(other) => panic!("{other:?}"),
    };
    assert_eq!(
        page(r#"{"task_id":"t-1","field":" task ","offset":2400}"#),
        Ok(Some(TaskPage { field: "task".into(), offset: 2400 }))
    );
    assert_eq!(
        page(r#"{"task_id":"t-1","field":"report.tests.2.details"}"#),
        Ok(Some(TaskPage { field: "report.tests.2.details".into(), offset: 0 }))
    );
    for (args, reason) in [
        (r#"{"task_id":"t-1","offset":5}"#, "`offset` needs `field`"),
        (r#"{"task_id":"t-1","field":"session"}"#, "`field` must be one of"),
        (r#"{"task_id":"t-1","field":"report.follow_ups.x"}"#, "`field` must be one of"),
        (r#"{"task_id":"t-1","field":"task","offset":-1}"#, "do not match the schema"),
        (r#"{"task_id":"t-1","field":3}"#, "do not match the schema"),
    ] {
        let got = page(args).unwrap_err();
        assert!(got.contains(reason), "{args}: {got}");
    }
}

#[test]
fn task_details_documents_paging_for_the_model() {
    let s = secretary(&SecretaryTool::ALL);
    let spec = specs(&s).into_iter().find(|t| t.name == "task_details").unwrap();
    assert!(spec.description.contains(
        "When `more` is present the task was not returned in full: before answering about a field listed in \
        `more`, read the rest with `field` and `offset` until `next_offset` is absent."
    ));
    assert!(!spec.description.contains("cut"));
    let properties = &spec.parameters["properties"];
    assert!(properties["field"]["description"].as_str().unwrap().contains("`report.follow_ups`"));
    assert_eq!(properties["offset"]["minimum"], 0);
}

#[test]
fn board_overview_takes_no_arguments() {
    let s = secretary(&SecretaryTool::ALL);
    assert_eq!(parse(&call("board_overview", ""), &s), Ok(Invocation::BoardOverview));
    assert_eq!(parse(&call("board_overview", "{}"), &s), Ok(Invocation::BoardOverview));
    assert!(parse(&call("board_overview", r#"{"workspace":"x"}"#), &s).is_err());
}

#[test]
fn the_new_read_tools_exist_only_for_a_secretary_who_has_them() {
    let old = secretary(&[SecretaryTool::BoardStatus, SecretaryTool::ThreadRead]);
    for name in ["task_details", "board_overview"] {
        assert_eq!(parse(&call(name, r#"{"task_id":"t-1"}"#), &old), Err(ToolRejection::NotAllowed(name.into())));
    }
    let names: Vec<&str> = specs(&old).iter().map(|t| t.name).collect();
    assert_eq!(names, ["board_status", "thread_read"]);
}

#[test]
fn the_bot_parameter_names_kinds_and_no_general_fallback() {
    let description = bot_parameter()["description"].as_str().unwrap().to_string();
    assert!(description.contains("kinds"));
    assert!(!description.contains("general"));
}
