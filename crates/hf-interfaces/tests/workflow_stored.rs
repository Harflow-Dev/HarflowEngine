//! Stored qualification: every shape table, fallbacks, observed paths and reference
//! identity, and the order of its checks.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use hf_domain::workflow::document::qualify_stored;
use hf_domain::workflow::json::Json;
use hf_domain::workflow::refusal::DocumentReason;
use hf_interfaces::workflow::json_text::parse;

const BASE: &str = r#"{"name":"flow","nodes":[{"id":"s","type":"start","position":{"x":1,"y":2},"agent":{"backend":"codex","fallbacks":[{"backend":"claude"}]}}],"connections":[{"id":"c","source":"s","target":"e"}],"orchestrator":{"backend":"codex","instructions":""},"on_demand_nodes":[{"id":"od"}]}"#;

fn base() -> Json {
    parse(BASE).unwrap()
}

/// `value` with `new` stored at the dotted path (numeric segments index arrays).
fn with_path(value: &Json, path: &[&str], new: Json) -> Json {
    let Some((head, rest)) = path.split_first() else {
        return new;
    };
    match value {
        Json::Object(map) => {
            let mut copy = map.clone();
            let inner = map.get(head).cloned().unwrap_or(Json::Null);
            copy.insert(head, with_path(&inner, rest, new));
            Json::Object(copy)
        }
        Json::Array(items) => {
            let mut copy = items.clone();
            let index: usize = head.parse().unwrap();
            copy[index] = with_path(&items[index], rest, new);
            Json::Array(copy)
        }
        _ => panic!("path {path:?} leaves the document"),
    }
}

fn qualify(value: &Json, source: Option<&str>) -> Result<(), DocumentReason> {
    let Json::Object(map) = value else {
        panic!("not an object")
    };
    qualify_stored(map, source)
}

fn malformed(path: &str) -> DocumentReason {
    DocumentReason::Malformed(path.to_string())
}

/// A value of the wrong kind for the field: an integer for lists, otherwise a list.
fn wrong(field_is_list: bool) -> Json {
    if field_is_list {
        Json::int(5)
    } else {
        Json::Array(Vec::new())
    }
}

const LISTS: [&str; 7] = [
    "nodes",
    "connections",
    "on_demand_nodes",
    "resources",
    "allowed_commands",
    "fallbacks",
    "observe_repos",
];

fn check_fields(prefix: &str, container: &[&str], fields: &[&str]) {
    for field in fields {
        let mut path = container.to_vec();
        path.push(field);
        let bad = with_path(&base(), &path, wrong(LISTS.contains(field)));
        let reason = qualify(&bad, None).unwrap_err();
        assert_eq!(
            reason,
            malformed(&format!("{prefix}.{field}")),
            "{prefix}.{field}"
        );
    }
}

#[test]
fn base_document_qualifies() {
    assert_eq!(qualify(&base(), None), Ok(()));
    assert_eq!(qualify(&base(), Some("flow")), Ok(()));
}

#[test]
fn every_definition_field_is_typed() {
    let fields = [
        "name",
        "workflow_id",
        "description",
        "routing_mode",
        "context_delivery",
        "execution_contract",
        "schema_version",
        "revision",
        "max_parallel",
        "max_transitions",
        "max_decision_attempts",
        "max_inspections",
        "max_on_demand_calls",
        "updated_at",
        "orchestrator",
        "nodes",
        "connections",
        "on_demand_nodes",
        "resources",
    ];
    check_fields("definition", &[], &fields);
}

#[test]
fn every_candidate_field_is_typed() {
    let fields = [
        "backend",
        "model",
        "reasoning_effort",
        "max_turns",
        "background_idle_seconds",
        "caller_approvals",
        "caller_write_approvals",
        "allowed_commands",
        "git_https_rewrite",
        "group_report_output",
        "fallbacks",
        "instructions",
    ];
    check_fields("agent", &["orchestrator"], &fields);
    check_fields("agent", &["nodes", "0", "agent"], &fields);
    check_fields("agent", &["nodes", "0", "agent", "fallbacks", "0"], &fields);
}

#[test]
fn every_node_field_is_typed() {
    let fields = [
        "id",
        "type",
        "title",
        "purpose",
        "instructions",
        "prompt",
        "role",
        "freedom",
        "session_mode",
        "execution_mode",
        "branch_mode",
        "branch_selection",
        "selection_guidance",
        "parallel_group_id",
        "join_id",
        "workflow_name",
        "orchestrator_mode",
        "child_session_policy",
        "call_condition",
        "optional",
        "require_technical_plan",
        "require_tasks",
        "always_route",
        "max_attempts",
        "max_context_questions",
        "network",
        "timeout_seconds",
        "agent",
        "position",
        "workflow_ref",
        "resources",
    ];
    check_fields("node", &["nodes", "0"], &fields);
    check_fields("node", &["on_demand_nodes", "0"], &fields);
}

#[test]
fn every_connection_and_position_field_is_typed() {
    check_fields(
        "connection",
        &["connections", "0"],
        &[
            "id",
            "source",
            "target",
            "condition",
            "default",
            "backward",
            "max_retries",
        ],
    );
    check_fields("position", &["nodes", "0", "position"], &["x", "y"]);
}

#[test]
fn exact_kinds_are_distinguished() {
    let at = |path: &[&str], value: Json| qualify(&with_path(&base(), path, value), None);
    assert_eq!(
        at(&["max_parallel"], Json::Bool(true)),
        Err(malformed("definition.max_parallel"))
    );
    assert_eq!(
        at(&["max_parallel"], Json::Float(1.0)),
        Err(malformed("definition.max_parallel"))
    );
    assert_eq!(at(&["updated_at"], Json::Float(1.5)), Ok(()));
    assert_eq!(at(&["updated_at"], Json::int(7)), Ok(()));
    assert_eq!(
        at(&["updated_at"], Json::Bool(false)),
        Err(malformed("definition.updated_at"))
    );
    assert_eq!(at(&["nodes", "0", "network"], Json::Null), Ok(()));
    assert_eq!(
        at(&["nodes", "0", "network"], Json::int(1)),
        Err(malformed("node.network"))
    );
    assert_eq!(at(&["nodes", "0", "timeout_seconds"], Json::Null), Ok(()));
    assert_eq!(
        at(&["nodes", "0", "position", "x"], Json::Float(2.5)),
        Ok(())
    );
    assert_eq!(
        at(&["nodes", "0", "position", "x"], Json::Bool(true)),
        Err(malformed("position.x"))
    );
    assert_eq!(at(&["orchestrator", "model"], Json::Null), Ok(()));
    assert_eq!(
        at(&["orchestrator", "max_turns"], Json::text("5")),
        Err(malformed("agent.max_turns"))
    );
}

#[test]
fn fallbacks_are_traversed() {
    let at = |path: &[&str], value: Json| qualify(&with_path(&base(), path, value), None);
    assert_eq!(
        at(
            &["orchestrator", "fallbacks"],
            Json::Array(vec![Json::int(1)])
        ),
        Err(DocumentReason::MalformedFallback)
    );
    let deep = parse(r#"[{"backend":"codex","fallbacks":[{"backend":"claude","max_turns":"x"}]}]"#)
        .unwrap();
    assert_eq!(
        at(&["orchestrator", "fallbacks"], deep),
        Err(malformed("agent.max_turns"))
    );
    let ok = parse(r#"[{"backend":"codex"},{"backend":"claude"}]"#).unwrap();
    assert_eq!(at(&["orchestrator", "fallbacks"], ok), Ok(()));
}

#[test]
fn observe_repos_and_references() {
    let at = |path: &[&str], value: &str| {
        qualify(&with_path(&base(), path, parse(value).unwrap()), None)
    };
    let repos = ["nodes", "0", "agent", "observe_repos"];
    assert_eq!(at(&repos, r#"["/a","/b"]"#), Ok(()));
    assert_eq!(
        at(&repos, r#"["a"]"#),
        Err(DocumentReason::MalformedObserveRepos)
    );
    assert_eq!(
        at(&repos, "[5]"),
        Err(DocumentReason::MalformedObserveRepos)
    );
    assert_eq!(
        at(&repos, "[\"\"]"),
        Err(DocumentReason::MalformedObserveRepos)
    );
    let reference = ["nodes", "0", "workflow_ref"];
    assert_eq!(at(&reference, r#"{"workflow_id":"w"}"#), Ok(()));
    for bad in [
        "{}",
        r#"{"workflow_id":""}"#,
        r#"{"workflow_id":"  "}"#,
        r#"{"workflow_id":5}"#,
    ] {
        assert_eq!(
            at(&reference, bad),
            Err(DocumentReason::MalformedReference),
            "{bad}"
        );
    }
    assert_eq!(at(&reference, "5"), Err(malformed("node.workflow_ref")));
}

#[test]
fn observe_repos_bounds_agree_at_seven() {
    let paths = |count: usize| {
        let list: Vec<String> = (0..count).map(|index| format!("\"/r{index}\"")).collect();
        format!("[{}]", list.join(","))
    };
    let repos = ["nodes", "0", "agent", "observe_repos"];
    let qualify_with = |count: usize| {
        qualify(
            &with_path(&base(), &repos, parse(&paths(count)).unwrap()),
            None,
        )
    };
    assert_eq!(qualify_with(7), Ok(()));
    assert_eq!(qualify_with(8), Err(DocumentReason::MalformedObserveRepos));
}

#[test]
fn non_object_entries() {
    let at = |path: &[&str], value: Json| qualify(&with_path(&base(), path, value), None);
    assert_eq!(
        at(&["nodes", "0"], Json::int(1)),
        Err(DocumentReason::MalformedNode)
    );
    assert_eq!(
        at(&["on_demand_nodes", "0"], Json::Null),
        Err(DocumentReason::MalformedNode)
    );
    assert_eq!(
        at(&["connections", "0"], Json::text("c")),
        Err(DocumentReason::MalformedConnection)
    );
}

#[test]
fn finite_values_everywhere() {
    for path in [
        vec!["extra"],
        vec!["nodes", "0", "weight"],
        vec!["orchestrator", "extra"],
        vec!["connections", "0", "extra"],
    ] {
        for token in ["NaN", "Infinity", "-Infinity"] {
            let nested = parse(&format!(r#"{{"deep":[1,{token}]}}"#)).unwrap();
            let bad = with_path(&base(), &path, nested);
            assert_eq!(
                qualify(&bad, None),
                Err(DocumentReason::NonFiniteValue),
                "{path:?} {token}"
            );
        }
    }
}

#[test]
fn name_match_only_with_a_source_name() {
    assert_eq!(
        qualify(&base(), Some("other")),
        Err(DocumentReason::NameMismatch)
    );
    assert_eq!(
        qualify(&base(), Some("bad/name")),
        Err(DocumentReason::InvalidName)
    );
    assert_eq!(qualify(&base(), None), Ok(()));
    let renamed = with_path(&base(), &["name"], Json::text("bad name "));
    assert_eq!(
        qualify(&renamed, Some("flow")),
        Err(DocumentReason::InvalidName)
    );
    assert_eq!(
        qualify(&renamed, None),
        Ok(()),
        "without a source name only the type is checked"
    );
    let numeric = with_path(&base(), &["name"], Json::int(5));
    assert_eq!(
        qualify(&numeric, Some("flow")),
        Err(DocumentReason::InvalidName)
    );
}

#[test]
fn qualification_order() {
    let nan = parse("NaN").unwrap();
    let both = with_path(
        &with_path(&base(), &["extra"], nan),
        &["max_parallel"],
        Json::text("x"),
    );
    assert_eq!(
        qualify(&both, Some("other")),
        Err(DocumentReason::NonFiniteValue)
    );
    let mismatch = with_path(&base(), &["max_parallel"], Json::text("x"));
    assert_eq!(
        qualify(&mismatch, Some("other")),
        Err(DocumentReason::NameMismatch)
    );
    assert_eq!(
        qualify(&mismatch, None),
        Err(malformed("definition.max_parallel"))
    );
    let first = with_path(
        &with_path(&base(), &["revision"], Json::text("x")),
        &["name"],
        Json::int(1),
    );
    assert_eq!(qualify(&first, None), Err(malformed("definition.name")));
}

#[test]
fn qualify_with_source_name_refuses_mismatch() {
    assert_eq!(qualify(&base(), Some("flow")), Ok(()));
    assert_eq!(
        qualify(&base(), Some("flow2")),
        Err(DocumentReason::NameMismatch)
    );
    assert_eq!(
        qualify(&base(), Some("Flow")),
        Err(DocumentReason::NameMismatch)
    );
    assert_eq!(
        qualify(&base(), Some("flow ")),
        Err(DocumentReason::InvalidName)
    );
}
