#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Parsing of the plan evidence of an artifact item of the fixture port.
use hf_interfaces::fixture_port::{ArtifactSpec, PlanSpec, Request, parse};
use serde_json::{Value, json};

const A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const PLAN: &str = "observed_backend_plan";

fn item(method: &str, extra: Value) -> Value {
    let mut value = json!({"artifact_id": "x1", "name": "f.txt", "target": "present",
        "kind": "report", "note": "n", "registered_at": "t", "provenance_method": method});
    for (key, entry) in extra.as_object().unwrap() {
        value[key] = entry.clone();
    }
    value
}

fn read(value: Value) -> Result<ArtifactSpec, String> {
    let body = json!({"task_id": A, "items": [value]}).to_string();
    match parse("write_artifacts", body.as_bytes()) {
        Ok(Request::WriteArtifacts { mut items, .. }) => Ok(items.remove(0)),
        Ok(other) => panic!("{other:?}"),
        Err(error) => Err(error.message().to_owned()),
    }
}

fn plan(extra: Value) -> Result<Option<PlanSpec>, String> {
    read(item(PLAN, extra)).map(|spec| spec.plan)
}

fn identity() -> Value {
    json!({"device": u64::MAX, "inode": 0})
}

#[test]
fn an_observed_plan_needs_a_tool_use_id_and_may_omit_the_directory() {
    assert_eq!(
        plan(json!({"tool_use_id": "t"})).unwrap(),
        Some(PlanSpec {
            tool_use_id: "t".into(),
            directory: None
        })
    );
    let error = plan(json!({})).unwrap_err();
    assert!(error.contains("tool_use_id"), "{error}");
    for bad in [json!(null), json!(7), json!("")] {
        assert!(plan(json!({"tool_use_id": bad})).is_err());
    }
}

#[test]
fn other_methods_carry_no_plan_keys() {
    let spec = read(item("verified_process_lineage", json!({}))).unwrap();
    assert_eq!(spec.plan, None);
    for (key, value) in [
        ("tool_use_id", json!("t")),
        ("tool_use_id", json!(null)),
        ("plan_directory", json!("/p")),
        ("plan_directory_identity", identity()),
    ] {
        let error = read(item("engine_output", json!({ key: value }))).unwrap_err();
        assert!(error.contains(key) && error.contains(PLAN), "{error}");
    }
}

#[test]
fn the_tool_use_id_is_one_to_256_bytes() {
    for (id, ok) in [
        ("x".repeat(256), true),
        ("x".repeat(257), false),
        ("\u{e9}".repeat(128), true),
        ("\u{e9}".repeat(129), false),
    ] {
        assert_eq!(plan(json!({"tool_use_id": id})).is_ok(), ok, "{}", id.len());
    }
}

#[test]
fn a_directory_comes_with_its_identity_and_back() {
    let directory = plan(json!({"tool_use_id": "t", "plan_directory": "/p/q",
        "plan_directory_identity": identity()}))
    .unwrap()
    .unwrap()
    .directory;
    assert_eq!(directory, Some(("/p/q".to_string(), u64::MAX, 0)));
    for extra in [
        json!({"tool_use_id": "t", "plan_directory": "/p"}),
        json!({"tool_use_id": "t", "plan_directory_identity": identity()}),
    ] {
        let error = plan(extra).unwrap_err();
        assert!(error.contains("come together"), "{error}");
    }
}

#[test]
fn the_directory_is_an_absolute_path_of_at_most_4096_bytes_without_parent_parts() {
    let identity = identity();
    let with = |path: String| {
        plan(json!({"tool_use_id": "t", "plan_directory": path,
            "plan_directory_identity": identity}))
    };
    let long = |n: usize| format!("/{}", "d".repeat(n - 1));
    assert!(with(long(4096)).is_ok());
    for path in [
        long(4097),
        String::new(),
        "relative/p".into(),
        "p".into(),
        "/a/../b".into(),
        "/..".into(),
        "/a/..".into(),
    ] {
        let error = with(path.clone()).unwrap_err();
        assert!(error.contains("plan_directory"), "{path}: {error}");
    }
    assert!(with("/a..b/c".into()).is_ok());
}

#[test]
fn the_identity_has_exactly_a_device_and_an_inode_of_unsigned_integers() {
    for bad in [
        json!({"device": 1}),
        json!({"inode": 1}),
        json!({"device": 1, "inode": 2, "size": 3}),
        json!({"device": -1, "inode": 2}),
        json!({"device": "1", "inode": 2}),
        json!({"device": 1.5, "inode": 2}),
        json!([1, 2]),
        json!(null),
    ] {
        let result = plan(json!({"tool_use_id": "t", "plan_directory": "/p",
            "plan_directory_identity": bad}));
        assert!(result.is_err(), "{bad}");
    }
}

#[test]
fn only_the_methods_and_kinds_a_python_manifest_can_hold_are_accepted() {
    for method in ["verified_process_lineage", "engine_output", PLAN] {
        let extra = if method == PLAN {
            json!({"tool_use_id": "t"})
        } else {
            json!({})
        };
        assert!(read(item(method, extra)).is_ok(), "{method}");
    }
    let error = read(item("other", json!({}))).unwrap_err();
    assert!(error.contains("provenance method"), "{error}");
    let mut odd_kind = item("engine_output", json!({}));
    odd_kind["kind"] = json!("k");
    let error = read(odd_kind).unwrap_err();
    assert!(error.contains("artifact kind"), "{error}");
}
