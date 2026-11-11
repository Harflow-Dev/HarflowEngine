#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Parsing of the chain-stop, launch and chain-root requests of the fixture port.
use hf_interfaces::fixture_port::{
    ChainRootSpec, LAUNCH_STATES, LaunchSpec, OPERATIONS, REFUSAL_CHECKS, Request, StopMode, parse,
};
use serde_json::{Value, json};

const A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

fn body(value: &Value) -> Vec<u8> {
    value.to_string().into_bytes()
}

fn stop(spec: Value) -> Result<StopMode, String> {
    match parse("stop_chain", &body(&json!({ "spec": spec }))) {
        Ok(Request::StopChain(parsed)) => {
            assert_eq!(parsed.chain_root.as_str(), A);
            Ok(parsed.mode)
        }
        Ok(other) => panic!("{other:?}"),
        Err(error) => Err(error.message().to_owned()),
    }
}

fn marker(kind: &str, cancelled: Value) -> Value {
    json!({"chain_root": A, "mode": "marker",
        "attribution": {"kind": kind, "cancelled_task_id": cancelled}})
}

fn launch_value() -> Value {
    json!({"name": A, "state": "waiting_for_resources", "generation": "c".repeat(32),
        "backend": "claude", "title": "t", "repo_path": "/r", "created_at": "c",
        "not_started_reason": null, "lineage": {}, "resources": {"names": ["x"]},
        "worktree": null, "refusal": null})
}

fn launch(spec: Value) -> Result<LaunchSpec, String> {
    match parse("add_launch", &body(&json!({ "spec": spec }))) {
        Ok(Request::AddLaunch(parsed)) => Ok(*parsed),
        Ok(other) => panic!("{other:?}"),
        Err(error) => Err(error.message().to_owned()),
    }
}

fn with(key: &str, value: Value) -> Value {
    let mut spec = launch_value();
    spec[key] = value;
    spec
}

#[test]
fn the_operation_list_names_the_three_new_operations() {
    assert_eq!(OPERATIONS.len(), 20);
    assert_eq!(
        &OPERATIONS[17..],
        ["stop_chain", "add_launch", "add_invalid_launch_name"]
    );
}

#[test]
fn stop_modes_and_attribution_kinds_parse() {
    assert_eq!(
        stop(json!({"chain_root": A, "mode": "absent"})),
        Ok(StopMode::Absent)
    );
    assert_eq!(
        stop(json!({"chain_root": A, "mode": "fault"})),
        Ok(StopMode::Fault)
    );
    assert_eq!(
        stop(marker("valid", json!(B))),
        Ok(StopMode::Marker {
            cancelled: Some(B.into())
        })
    );
    assert_eq!(
        stop(marker("invalid", json!("not an id!"))),
        Ok(StopMode::Marker {
            cancelled: Some("not an id!".into())
        })
    );
    assert_eq!(
        stop(marker("malformed", Value::Null)),
        Ok(StopMode::Marker { cancelled: None })
    );
    let Ok(StopMode::Marker {
        cancelled: Some(text),
    }) = stop(marker("oversize", json!(B)))
    else {
        panic!("oversize");
    };
    assert!(text.starts_with(B) && text.len() == B.len() + 4096);
}

#[test]
fn malformed_stop_requests_are_refused() {
    for spec in [
        json!({"chain_root": A, "mode": "other"}),
        json!({"chain_root": A}),
        json!({"mode": "absent"}),
        json!({"chain_root": "bad id", "mode": "absent"}),
        json!({"chain_root": A, "mode": "absent", "extra": 1}),
        json!({"chain_root": A, "mode": "marker"}),
        marker("valid", json!("not an id!")),
        marker("valid", Value::Null),
        marker("invalid", Value::Null),
        marker("oversize", Value::Null),
        marker("other", json!(B)),
    ] {
        assert!(stop(spec.clone()).is_err(), "{spec}");
    }
}

#[test]
fn a_complete_launch_parses_with_its_lineage_and_shapes() {
    let mut spec = launch_value();
    spec["lineage"] = json!({"spawned_by": "s", "chain_root_task_id": "junk!"});
    spec["worktree"] = json!({"path": ""});
    let parsed = launch(spec).unwrap();
    assert_eq!(parsed.name.as_str(), A);
    assert_eq!(parsed.lineage.spawned_by.as_deref(), Some("s"));
    assert_eq!(parsed.lineage.chain_root_task_id.as_deref(), Some("junk!"));
    assert_eq!(parsed.lineage.root_task_id, None);
    assert_eq!(parsed.resources.unwrap().names, Some(vec!["x".to_owned()]));
    assert_eq!(parsed.worktree.unwrap().path.as_deref(), Some(""));
    assert_eq!(parsed.refusal, None);
}

#[test]
fn empty_objects_and_non_list_names_carry_no_inner_value() {
    for (resources, names) in [
        (json!({}), None),
        (json!({"names": []}), Some(vec![])),
        (json!({"names": "x"}), None),
        (json!({"names": ["a", 1]}), None),
    ] {
        let parsed = launch(with("resources", resources.clone())).unwrap();
        assert_eq!(parsed.resources.unwrap().names, names, "{resources}");
    }
    assert_eq!(
        launch(with("resources", Value::Null)).unwrap().resources,
        None
    );
    assert_eq!(
        launch(with("worktree", json!({})))
            .unwrap()
            .worktree
            .unwrap()
            .path,
        None
    );
}

#[test]
fn every_state_and_refusal_check_is_accepted_and_others_are_refused() {
    for state in LAUNCH_STATES {
        assert!(launch(with("state", json!(state))).is_ok(), "{state}");
    }
    for check in REFUSAL_CHECKS {
        let parsed = launch(with("refusal", json!({"check": check}))).unwrap();
        assert_eq!(parsed.refusal.as_deref(), Some(check));
    }
    assert!(launch(with("state", json!("running"))).is_err());
    assert!(launch(with("refusal", json!({"check": "other"}))).is_err());
    assert!(launch(with("refusal", json!({}))).is_err());
}

#[test]
fn malformed_launch_requests_are_refused() {
    assert!(launch(with("name", json!("not an id"))).is_err());
    assert!(launch(with("resources", json!("x"))).is_err());
    assert!(launch(with("lineage", json!({"unknown": "x"}))).is_err());
    assert!(launch(with("lineage", json!({"spawned_by": 3}))).is_err());
    let mut missing = launch_value();
    missing.as_object_mut().unwrap().remove("refusal");
    assert!(launch(missing).is_err());
    let mut extra = launch_value();
    extra["extra"] = json!(1);
    assert!(launch(extra).is_err());
}

#[test]
fn invalid_launch_names_must_not_be_task_identities() {
    let name = |text: &str| parse("add_invalid_launch_name", &body(&json!({ "name": text })));
    assert!(matches!(
        name("bad name"),
        Ok(Request::AddInvalidLaunchName(text)) if text == "bad name"
    ));
    for refused in [A, "", "a/b", "a\0b"] {
        assert!(name(refused).is_err(), "{refused:?}");
    }
}

fn task_spec(extra: Value) -> Value {
    let mut spec = json!({"task_id": A, "backend": "claude", "repo_path": "/r",
        "freedom": "write_in_repo", "session_id": "s", "status": "completed", "exit_code": 0,
        "started_at": "2026-01-10T10:00:00+00:00", "finished_at": "2026-01-10T10:05:00+00:00",
        "output_drained": true, "prompt": "p"});
    for (key, value) in extra.as_object().unwrap() {
        spec[key] = value.clone();
    }
    spec
}

fn chain_root(extra: Value) -> Result<ChainRootSpec, String> {
    match parse("add_task", &body(&json!({"spec": task_spec(extra)}))) {
        Ok(Request::AddTask(spec)) => Ok(spec.chain_root),
        Ok(other) => panic!("{other:?}"),
        Err(error) => Err(error.message().to_owned()),
    }
}

#[test]
fn an_add_task_chain_root_is_default_absent_or_verbatim() {
    assert_eq!(chain_root(json!({})), Ok(ChainRootSpec::Default));
    assert_eq!(
        chain_root(json!({"chain_root": null})),
        Ok(ChainRootSpec::Absent)
    );
    assert_eq!(
        chain_root(json!({"chain_root": "not an id!"})),
        Ok(ChainRootSpec::Verbatim("not an id!".into()))
    );
    assert_eq!(
        chain_root(json!({"chain_root": ""})),
        Ok(ChainRootSpec::Verbatim(String::new()))
    );
    assert!(chain_root(json!({"chain_root": 7})).is_err());
}

#[test]
fn the_header_blob_carries_the_resolved_chain_root() {
    let Ok(Request::AddTask(spec)) = parse(
        "add_task",
        &body(&json!({"spec": task_spec(json!({"continuation_of": B}))})),
    ) else {
        panic!("add_task");
    };
    let header: Value = serde_json::from_slice(&spec.header_blob(Some("root!"))).unwrap();
    assert_eq!(header["chain_root_task_id"], "root!");
    assert_eq!(header["continuation_of_task_id"], B);
    let header: Value = serde_json::from_slice(&spec.header_blob(None)).unwrap();
    assert_eq!(header["chain_root_task_id"], Value::Null);
}
