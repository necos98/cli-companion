use super::*;
use serde_json::json;

fn ws() -> Workspace {
    let raw = r#"{"id":"ws-1","name":"p","kind":"windows","path":"C:\\p","created_at":1}"#;
    serde_json::from_str(raw).unwrap()
}

fn launch(id: &str) -> Launch {
    Launch { id: id.into(), name: format!("name {id}"), command: format!("cmd {id}"), is_default: false }
}

fn patch(v: serde_json::Value) -> SettingsPatch {
    SettingsPatch::parse(&v).unwrap()
}

fn id_gen() -> impl FnMut() -> String {
    let mut n = 0;
    move || {
        n += 1;
        format!("new-{n}")
    }
}

fn apply(p: &SettingsPatch, w: &mut Workspace) -> Result<(), String> {
    p.apply(w, &mut id_gen())
}

#[test]
fn parse_distinguishes_absent_from_null() {
    let p = patch(json!({"worker_model": null, "default_launch": null, "repos": {"a": null, "b": {"base_branch": null}}}));
    assert_eq!(p.worker_model, Some(None));
    assert_eq!(p.default_launch, Some(None));
    assert_eq!(p.worker_rules, None);
    let repos = p.repos.unwrap();
    assert_eq!(repos["a"], None);
    assert_eq!(repos["b"].as_ref().unwrap().base_branch, Some(None));
    assert_eq!(repos["b"].as_ref().unwrap().container_root, None);
    let p = patch(json!({"worker_model": "opus"}));
    assert_eq!(p.worker_model, Some(Some("opus".to_string())));
}

#[test]
fn parse_roundtrips_absent_and_null() {
    let v = json!({"worker_model": null, "repos": {"a": null, "b": {"base_branch": null}}});
    assert_eq!(serde_json::to_value(patch(v.clone())).unwrap(), v);
}

#[test]
fn parse_empty_object_is_empty_patch() {
    assert!(patch(json!({})).is_empty());
    assert!(!patch(json!({"use_schedule": false})).is_empty());
}

#[test]
fn parse_rejects_unknown_fields_and_non_objects() {
    let e = SettingsPatch::parse(&json!({"theme": "dark"})).unwrap_err();
    assert!(e.starts_with("invalid settings: ") && e.contains("theme"), "{e}");
    assert!(SettingsPatch::parse(&json!({"repos": {"a": {"nope": 1}}})).is_err());
    assert!(SettingsPatch::parse(&json!({"delivery": {"nope": 1}})).is_err());
    assert!(SettingsPatch::parse(&json!({"launches": [{"name": "a", "command": "b", "x": 1}]})).is_err());
    for v in [json!([]), json!("x"), json!(null), json!(3)] {
        let e = SettingsPatch::parse(&v).unwrap_err();
        assert!(e.contains("expected a JSON object"), "{e}");
    }
}

#[test]
fn parse_rejects_fixed_fields_with_clear_message() {
    for field in ["id", "name", "path", "kind", "distro", "shell", "archived", "created_at"] {
        let e = SettingsPatch::parse(&json!({ field: "x", "use_schedule": true })).unwrap_err();
        assert!(e.starts_with(&format!("`{field}` cannot be changed by a proposal")), "{e}");
        assert!(e.contains("fixed: id, name, path, kind, distro, shell, archived, created_at"), "{e}");
    }
}

#[test]
fn normalized_cleans_values() {
    let mut w = ws();
    w.launches = vec![launch("l1")];
    let p = patch(json!({
        "worker_rules": "  no tests  ",
        "worker_model": "  ",
        "worker_permission": " accept-edits ",
        "worker_allow": [" Bash(git:*) ", "", "Bash(git:*)", "Read"],
        "worker_deny": ["WebFetch"],
        "delivery": {"mode": "commit", "parallel": true},
        "use_schedule": true,
        "default_launch": " l1 ",
        "launches": [{"id": "l1", "name": " Dev ", "command": " ls "}],
        "repos": {"api/": {"base_branch": " main ", "setup": [" a ", ""]}}
    }));
    let n = p.normalized(&w).unwrap();
    assert_eq!(n.worker_rules.as_deref(), Some("no tests"));
    assert_eq!(n.worker_model, Some(None));
    assert_eq!(n.worker_permission.as_deref(), Some("accept-edits"));
    assert_eq!(n.worker_allow, Some(vec!["Bash(git:*)".to_string(), "Read".to_string()]));
    assert_eq!(n.worker_deny, Some(vec!["WebFetch".to_string()]));
    // `parallel` non ha senso senza branch: scartato.
    assert_eq!(n.delivery, Some(DeliveryPatch { mode: Some(DeliveryMode::Commit), parallel: Some(false), merge_method: None }));
    assert_eq!(n.use_schedule, Some(true));
    assert_eq!(n.default_launch, Some(Some("l1".to_string())));
    assert_eq!(n.launches, Some(vec![LaunchPatch { id: Some("l1".into()), name: "Dev".into(), command: "ls".into() }]));
    let api = n.repos.unwrap()["api"].clone().unwrap();
    assert_eq!(api.base_branch, Some(Some("main".to_string())));
    assert_eq!(api.setup, Some(vec!["a".to_string()]));
    assert_eq!(api.cleanup, None);
}

#[test]
fn delivery_keeps_parallel_with_branches_and_merges_onto_current() {
    let mut w = ws();
    w.delivery = Delivery { mode: DeliveryMode::Pr, parallel: true, merge_method: MergeMethod::Merge };
    let n = patch(json!({"delivery": {"merge_method": "rebase"}})).normalized(&w).unwrap();
    assert_eq!(n.delivery, Some(DeliveryPatch { merge_method: Some(MergeMethod::Rebase), ..Default::default() }));
    // Cambiando solo la modalita', `parallel` esistente viene riletto dalla normalizzazione.
    let n = patch(json!({"delivery": {"mode": "push"}})).normalized(&w).unwrap();
    assert_eq!(n.delivery, Some(DeliveryPatch { mode: Some(DeliveryMode::Push), ..Default::default() }));
    let mut next = w.clone();
    patch(json!({"delivery": {"mode": "push"}})).apply(&mut next, &mut id_gen()).unwrap();
    assert!(!next.delivery.parallel);
}

#[test]
fn invalid_values_fail_and_translate_to_english() {
    let mut w = ws();
    w.launches = vec![launch("l1")];
    let too_many_launches: Vec<_> = (0..=MAX_PATCH_LAUNCHES).map(|i| json!({"name": format!("n{i}"), "command": "c"})).collect();
    let cases = [
        (json!({"worker_rules": "x".repeat(MAX_RULES_LEN + 1)}), "worker_rules"),
        (json!({"worker_rules": "a\u{1}b"}), "worker_rules"),
        (json!({"worker_model": "no good!"}), "worker_model"),
        (json!({"worker_permission": "root"}), "worker_permission"),
        (json!({"worker_allow": ["bad rule"]}), "worker_allow: rule \"bad rule\""),
        (json!({"worker_allow": ["a\nb"]}), "worker_allow: rule"),
        (json!({"worker_deny": ["mcp__srv__tool(x)"]}), "worker_deny: rule \"mcp__srv__tool(x)\": mcp__ rules take no parentheses"),
        (json!({"worker_deny": [format!("Bash({})", "a".repeat(MAX_RULE_CHARS))]}), "worker_deny: rule"),
        (json!({"worker_allow": (0..=MAX_RULES).map(|i| format!("R{i}")).collect::<Vec<_>>()}), "worker_allow: too many rules"),
        (json!({"delivery": {}}), "delivery"),
        (json!({"launches": [{"name": "", "command": "c"}]}), "launches[0]: launch name is missing"),
        (json!({"launches": [{"name": "n".repeat(MAX_LAUNCH_NAME_LEN + 1), "command": "c"}]}), "launches[0]: launch name too long"),
        (json!({"launches": [{"name": "n", "command": "c"}, {"name": "n", "command": " "}]}), "launches[1]: launch command is missing"),
        (json!({"launches": [{"id": "ghost", "name": "n", "command": "c"}]}), "launches[0]: launch not found: an id must be"),
        (json!({"launches": [{"id": "l1", "name": "n", "command": "c"}, {"id": "l1", "name": "m", "command": "c"}]}), "launches[1]: duplicate launch id"),
        (json!({"launches": too_many_launches}), "too many launches"),
        (json!({"default_launch": "nope"}), "invalid default_launch"),
        (json!({"repos": {"x": {"base_branch": "no good"}}}), "repos['x']: invalid base_branch: no good"),
        (json!({"repos": {"x": {"container_root": "a\nb"}}}), "repos['x']: invalid container_root"),
        (json!({"repos": {"x": {"copy_ignored": ["/abs"]}}}), "repos['x']: copy_ignored paths must be relative"),
        (json!({"repos": {"x": {"copy_ignored": [".."]}}}), "repos['x']: invalid copy_ignored path"),
        (json!({"repos": {"x": {"setup": ["a\nb"]}}}), "repos['x']: setup commands must be single-line"),
        (json!({"repos": {"x": {"cleanup": ["a\nb"]}}}), "repos['x']: cleanup commands must be single-line"),
        (json!({"repos": {"x": {"setup": ["y".repeat(MAX_REPO_COMMAND_LEN + 1)]}}}), "repos['x']: setup command too long"),
        (json!({"repos": {"x": {"cleanup": vec!["y"; MAX_REPO_COMMANDS + 1]}}}), "repos['x']: too many cleanup commands"),
        (json!({"repos": {"x": {"copy_ignored": (0..=MAX_REPO_COMMANDS).map(|i| format!("p{i}")).collect::<Vec<_>>()}}}), "repos['x']: too many copy_ignored paths"),
        (json!({"repos": {"x": {}}}), "repos['x']: has no fields"),
        (json!({"repos": {"x": {"remove_force": true}, "x/": {"remove_force": true}}}), "duplicate repo key"),
        (json!({"repos": {"/abs": {"remove_force": true}}}), "repos['/abs']"),
    ];
    for (v, expect) in cases {
        let err = patch(v.clone()).normalized(&w).unwrap_err();
        let en = validation_error_en(&err);
        assert!(en.contains(expect), "{v}: {en} (it: {err})");
    }
}

#[test]
fn italian_messages_are_translated_not_passed_through() {
    let w = ws();
    for v in [
        json!({"worker_rules": "a\u{1}b"}),
        json!({"worker_permission": "root"}),
        json!({"worker_allow": ["bad rule"]}),
        json!({"launches": [{"name": "", "command": "c"}]}),
        json!({"repos": {"x": {"base_branch": "no good"}}}),
    ] {
        let err = patch(v).normalized(&w).unwrap_err();
        assert_ne!(validation_error_en(&err), err);
    }
    for already_english in ["too many repos in the proposal (max 20)", "something unknown", ""] {
        assert_eq!(validation_error_en(already_english), already_english);
    }
}

#[test]
fn rule_errors_keep_limits_in_english() {
    assert_eq!(
        validation_error_en(&format!("Vietati: regola \"x\": piu' lunga di {MAX_RULE_CHARS} caratteri")),
        format!("worker_deny: rule \"x\": longer than {MAX_RULE_CHARS} characters")
    );
    assert_eq!(
        validation_error_en("Consentiti: regola \"a b\": forma attesa: Nome oppure Nome(...), es. Bash(git:*)"),
        "worker_allow: rule \"a b\": expected Name or Name(...), e.g. Bash(git:*)"
    );
}

#[test]
fn default_launch_is_checked_against_launches_after_the_patch() {
    let mut w = ws();
    w.launches = vec![launch("l1"), launch("l2")];
    // l2 viene tolto dalla lista proposta: non puo' essere il predefinito.
    let both = patch(json!({"launches": [{"id": "l1", "name": "a", "command": "b"}], "default_launch": "l2"}));
    assert!(both.normalized(&w).is_err());
    let ok = patch(json!({"launches": [{"id": "l1", "name": "a", "command": "b"}], "default_launch": "l1"}));
    assert!(ok.normalized(&w).is_ok());
    // "shell" e gli integrati sono sempre ammessi.
    assert!(patch(json!({"default_launch": "shell"})).normalized(&w).is_ok());
    assert!(patch(json!({"default_launch": "builtin:claude-code"})).normalized(&w).is_ok());
}

#[test]
fn apply_is_all_or_nothing() {
    let mut w = ws();
    w.launches = vec![launch("l1")];
    let before = w.clone();
    let p = patch(json!({
        "worker_rules": "valid",
        "use_schedule": true,
        "repos": {"a": {"setup": ["ok"]}, "b": {"base_branch": "no good"}}
    }));
    assert!(apply(&p, &mut w).is_err());
    assert_eq!(w, before);
}

#[test]
fn apply_changes_everything_at_once() {
    let mut w = ws();
    w.launches = vec![launch("l1"), launch("l2")];
    w.default_launch = Some("l2".into());
    w.repos.insert("api".into(), RepoConfig { base_branch: Some("main".into()), ..Default::default() });
    w.repos.insert("old".into(), RepoConfig::default());
    let p = patch(json!({
        "worker_rules": "rules",
        "worker_model": "opus",
        "worker_permission": "plan",
        "worker_allow": ["Read"],
        "worker_deny": ["WebFetch"],
        "delivery": {"mode": "local_merge", "parallel": true},
        "use_schedule": true,
        "launches": [{"name": "fresh", "command": "c"}, {"id": "l1", "name": "renamed", "command": "x"}],
        "repos": {"api": {"base_branch": null, "setup": ["s"]}, "old": null, "new": {"remove_force": true}}
    }));
    apply(&p, &mut w).unwrap();
    assert_eq!(w.worker_rules, "rules");
    assert_eq!(w.worker_model.as_deref(), Some("opus"));
    assert_eq!(w.worker_permission, "plan");
    assert_eq!((w.worker_allow.as_slice(), w.worker_deny.as_slice()), (&["Read".to_string()][..], &["WebFetch".to_string()][..]));
    assert_eq!(w.delivery, Delivery { mode: DeliveryMode::LocalMerge, parallel: true, merge_method: MergeMethod::Squash });
    assert!(w.use_schedule);
    // Nuovo avvio con id generato, l1 mantiene l'id, l2 e' sparito (era il predefinito).
    let ids: Vec<_> = w.launches.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(ids, ["new-1", "l1"]);
    assert_eq!(w.launches[1].name, "renamed");
    assert_eq!(w.default_launch, None);
    assert_eq!(w.repos["api"].base_branch, None);
    assert_eq!(w.repos["api"].setup, ["s"]);
    assert!(!w.repos.contains_key("old"));
    assert!(w.repos["new"].remove_force);
}

#[test]
fn apply_default_launch_variants() {
    let mut w = ws();
    w.launches = vec![launch("l1"), launch("l2")];
    w.default_launch = Some("l1".into());
    // Il predefinito resta se l'avvio resta.
    apply(&patch(json!({"launches": [{"id": "l1", "name": "a", "command": "b"}]})), &mut w).unwrap();
    assert_eq!(w.default_launch.as_deref(), Some("l1"));
    // Un predefinito integrato non viene toccato dalla sostituzione della lista.
    w.default_launch = Some("shell".into());
    apply(&patch(json!({"launches": []})), &mut w).unwrap();
    assert!(w.launches.is_empty());
    assert_eq!(w.default_launch.as_deref(), Some("shell"));
    // Impostazione esplicita e ritorno all'ereditarieta'.
    w.launches = vec![launch("l1")];
    apply(&patch(json!({"default_launch": "l1"})), &mut w).unwrap();
    assert_eq!(w.default_launch.as_deref(), Some("l1"));
    apply(&patch(json!({"default_launch": null})), &mut w).unwrap();
    assert_eq!(w.default_launch, None);
}

#[test]
fn apply_absent_fields_change_nothing() {
    let mut w = ws();
    w.worker_rules = "keep".into();
    w.launches = vec![launch("l1")];
    w.default_launch = Some("l1".into());
    w.repos.insert("api".into(), RepoConfig { setup: vec!["s".into()], ..Default::default() });
    let before = w.clone();
    apply(&patch(json!({})), &mut w).unwrap();
    assert_eq!(w, before);
    apply(&patch(json!({"repos": {"api": {"remove_force": true}}})), &mut w).unwrap();
    assert_eq!(w.repos["api"].setup, ["s"]);
    assert!(w.repos["api"].remove_force);
    assert_eq!(w.worker_rules, "keep");
}

#[test]
fn apply_repo_null_values_clear_or_remove() {
    let mut w = ws();
    w.repos.insert(
        ".".into(),
        RepoConfig { base_branch: Some("dev".into()), container_root: Some("/srv".into()), ..Default::default() },
    );
    apply(&patch(json!({"repos": {".": {"container_root": null}}})), &mut w).unwrap();
    assert_eq!((w.repos["."].base_branch.as_deref(), w.repos["."].container_root.as_deref()), (Some("dev"), None));
    apply(&patch(json!({"repos": {"./": null}})), &mut w).unwrap();
    assert!(w.repos.is_empty());
    // Rimuovere un repository senza configurazione e' un no-op.
    apply(&patch(json!({"repos": {"ghost": null}})), &mut w).unwrap();
}

#[test]
fn current_reports_only_touched_fields() {
    let mut w = ws();
    w.worker_rules = "old".into();
    w.worker_model = Some("sonnet".into());
    w.worker_allow = vec!["Read".into()];
    w.launches = vec![launch("l1")];
    w.default_launch = Some("l1".into());
    w.delivery = Delivery { mode: DeliveryMode::Pr, parallel: true, merge_method: MergeMethod::Merge };
    w.repos.insert("api".into(), RepoConfig { base_branch: Some("main".into()), setup: vec!["a".into()], ..Default::default() });
    w.repos.insert("gone".into(), RepoConfig { cleanup: vec!["c".into()], remove_force: true, ..Default::default() });
    let p = patch(json!({
        "worker_rules": "new",
        "worker_model": null,
        "worker_allow": ["Bash"],
        "delivery": {"mode": "commit"},
        "default_launch": null,
        "launches": [],
        "repos": {"api": {"setup": ["b"]}, "fresh": {"setup": ["x"]}, "gone": null}
    }))
    .normalized(&w)
    .unwrap();
    let cur = p.current(&w);
    assert_eq!(cur.worker_rules.as_deref(), Some("old"));
    assert_eq!(cur.worker_model, Some(Some("sonnet".to_string())));
    assert_eq!(cur.worker_allow, Some(vec!["Read".to_string()]));
    assert_eq!(cur.delivery, Some(DeliveryPatch { mode: Some(DeliveryMode::Pr), ..Default::default() }));
    assert_eq!(cur.default_launch, Some(Some("l1".to_string())));
    assert_eq!(cur.launches, Some(vec![LaunchPatch { id: Some("l1".into()), name: "name l1".into(), command: "cmd l1".into() }]));
    // Campi non toccati: assenti.
    assert!(cur.worker_deny.is_none() && cur.worker_permission.is_none() && cur.use_schedule.is_none());
    let repos = cur.repos.unwrap();
    assert_eq!(repos["api"], Some(RepoPatch { setup: Some(vec!["a".to_string()]), ..Default::default() }));
    assert_eq!(repos["fresh"], None);
    let gone = repos["gone"].clone().unwrap();
    assert_eq!(gone.cleanup, Some(vec!["c".to_string()]));
    assert_eq!(gone.remove_force, Some(true));
    assert_eq!(gone.base_branch, Some(None));
    assert!(gone.copy_ignored.is_some() && gone.setup.is_some() && gone.container_root.is_some());
    // Una patch di un solo campo non riporta altro.
    assert_eq!(patch(json!({"use_schedule": true})).current(&w), SettingsPatch { use_schedule: Some(false), ..Default::default() });
}

#[test]
fn sensitive_lists_commands_and_permissions_in_order() {
    let all = patch(json!({
        "worker_rules": "r",
        "worker_model": "m",
        "worker_permission": "bypass",
        "worker_allow": [],
        "worker_deny": [],
        "delivery": {"mode": "push"},
        "use_schedule": true,
        "default_launch": null,
        "launches": [],
        "repos": {"api/web": {"cleanup": ["c"]}, ".": {"setup": ["s"], "cleanup": ["c"], "remove_force": true}, "gone": null, "plain": {"base_branch": "x"}}
    }));
    assert_eq!(
        all.sensitive(),
        [
            "worker_permission",
            "worker_allow",
            "worker_deny",
            "launches",
            "default_launch",
            "repos[.].setup",
            "repos[.].cleanup",
            "repos[api/web].cleanup",
        ]
    );
}

#[test]
fn sensitive_is_empty_for_harmless_patches() {
    let p = patch(json!({
        "worker_rules": "r",
        "worker_model": null,
        "delivery": {"parallel": true},
        "use_schedule": false,
        "repos": {"a": null, "b": {"base_branch": "x", "container_root": "/srv", "copy_ignored": [".env"], "remove_force": true}}
    }));
    assert!(p.sensitive().is_empty());
    assert!(patch(json!({})).sensitive().is_empty());
}

#[test]
fn needs_reconcile_and_repo_keys() {
    assert!(patch(json!({"delivery": {"mode": "push"}})).needs_reconcile());
    assert!(patch(json!({"use_schedule": false})).needs_reconcile());
    assert!(!patch(json!({"worker_rules": "x", "repos": {"a": null}})).needs_reconcile());
    let w = ws();
    let n = patch(json!({"repos": {"b/": {"remove_force": true}, "./": null, "a": null}})).normalized(&w).unwrap();
    assert_eq!(n.repo_keys(), [".", "a", "b"]);
    assert!(patch(json!({})).repo_keys().is_empty());
}

#[test]
fn e2e_is_set_kept_and_removed_by_a_patch() {
    let mut w = ws();
    let set = json!({"repos": {"api": {"e2e": {"command": " npm run e2e ", "junit": ".\\out\\junit.xml"}}}});
    apply(&patch(set), &mut w).unwrap();
    let e2e = w.repos["api"].e2e.clone().unwrap();
    assert_eq!((e2e.command.as_str(), e2e.junit.as_str(), e2e.timeout_min), ("npm run e2e", "out/junit.xml", 20));
    // Assente: resta; altri campi non lo toccano.
    apply(&patch(json!({"repos": {"api": {"remove_force": true}}})), &mut w).unwrap();
    assert_eq!(w.repos["api"].e2e.as_ref(), Some(&e2e));
    // null: lo rimuove.
    apply(&patch(json!({"repos": {"api": {"e2e": null}}})), &mut w).unwrap();
    assert!(w.repos["api"].e2e.is_none());
    // Invalido: nulla cambia.
    let before = w.clone();
    let bad = json!({"repos": {"api": {"e2e": {"command": "x", "junit": "../j.xml"}}}});
    assert!(apply(&patch(bad), &mut w).is_err());
    assert_eq!(w, before);
    assert!(SettingsPatch::parse(&json!({"repos": {"api": {"e2e": {"command": "x", "junit": "j", "cwd": "."}}}})).is_err());
    assert!(SettingsPatch::parse(&json!({"repos": {"api": {"e2e": {"command": "x"}}}})).is_err());
}

#[test]
fn e2e_current_and_sensitive() {
    let mut w = ws();
    let cfg = crate::workspaces::E2eConfig { command: "c".into(), junit: "j.xml".into(), timeout_min: 5 };
    w.repos.insert("api".into(), RepoConfig { e2e: Some(cfg.clone()), ..Default::default() });
    let set = patch(json!({"repos": {"api": {"e2e": {"command": "d", "junit": "j.xml"}}, "web": {"e2e": null}}}));
    assert_eq!(set.sensitive(), ["repos[api].e2e"], "a removal is not sensitive");
    let cur = set.current(&w);
    assert_eq!(cur.repos.as_ref().unwrap()["api"].as_ref().unwrap().e2e, Some(Some(cfg.clone())));
    assert_eq!(cur.repos.as_ref().unwrap()["web"], None);
    // Rimuovere un repository mostra l'e2e attuale tra i valori persi.
    let gone = patch(json!({"repos": {"api": null}})).current(&w);
    assert_eq!(gone.repos.unwrap()["api"].as_ref().unwrap().e2e, Some(Some(cfg)));
}

#[test]
fn e2e_errors_are_translated() {
    let w = ws();
    let cases = [
        (json!({"command": "  ", "junit": "j"}), "repos['x']: e2e command is missing"),
        (json!({"command": "a\nb", "junit": "j"}), "repos['x']: e2e command must be single-line"),
        (json!({"command": "y".repeat(MAX_REPO_COMMAND_LEN + 1), "junit": "j"}), "repos['x']: e2e command too long"),
        (json!({"command": "c", "junit": ""}), "repos['x']: e2e junit path is missing"),
        (json!({"command": "c", "junit": "/abs"}), "repos['x']: e2e junit path must be relative"),
        (json!({"command": "c", "junit": "../j"}), "repos['x']: invalid e2e junit path (no '..')"),
        (json!({"command": "c", "junit": "j", "timeout_min": 0}), "repos['x']: invalid e2e timeout_min"),
        (json!({"command": "c", "junit": "j", "timeout_min": 61}), "repos['x']: invalid e2e timeout_min"),
    ];
    for (e2e, expect) in cases {
        let err = patch(json!({"repos": {"x": {"e2e": e2e}}})).normalized(&w).unwrap_err();
        let en = validation_error_en(&err);
        assert!(en.contains(expect), "{e2e}: {en} (it: {err})");
        assert_ne!(en, err);
    }
}
