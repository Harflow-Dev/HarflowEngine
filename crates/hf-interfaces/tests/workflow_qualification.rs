//! Validation qualifies what it accepts the way a save would store it: definitions that
//! could not be read back are refused with the bare stored-document reason, and fields the
//! store overwrites are never refused.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use hf_domain::workflow::json::Json;

fn end_with(fields: &str) -> String {
    node_flow("", r#"{"id":"a","type":"agent"}"#).replace(
        r#"{"id":"e","type":"end"}"#,
        &format!(r#"{{"id":"e","type":"end",{fields}}}"#),
    )
}

fn start_with(fields: &str) -> String {
    agent("").replace(
        r#"{"id":"s","type":"start"}"#,
        &format!(r#"{{"id":"s","type":"start",{fields}}}"#),
    )
}

#[test]
fn p12_validate_refuses_what_decode_refuses() {
    for network in ["0", "1", "1.0", r#""yes""#] {
        assert_eq!(
            refused(&end_with(&format!(r#""network":{network}"#))),
            "Malformed node.network",
            "{network}"
        );
    }
    assert_eq!(refused(&end_with(r#""prompt":5"#)), "Malformed node.prompt");
    assert_eq!(refused(&agent(r#""prompt":5"#)), "Malformed node.prompt");
    assert_eq!(refused(&start_with(r#""role":5"#)), "Malformed node.role");
    assert_eq!(
        refused(&start_with(r#""agent":"x""#)),
        "Malformed node.agent"
    );
    assert_eq!(
        refused(&agent(r#""parallel_group_id":5"#)),
        "Malformed node.parallel_group_id"
    );
    assert_eq!(
        refused(&agent(r#""workflow_ref":{"workflow_id":""}"#)),
        "Malformed workflow reference identity"
    );
    assert_eq!(
        refused(&top(r#""routing_mode":null"#)),
        "Malformed definition.routing_mode"
    );
    assert_eq!(
        refused(&top(r#""extra":NaN"#)),
        "Definition must contain finite JSON values"
    );
    assert_eq!(
        refused(&top(r#""extra":[Infinity]"#)),
        "Definition must contain finite JSON values"
    );
}

#[test]
fn more_unchecked_node_fields_are_typed() {
    for (fields, reason) in [
        (r#""max_attempts":"x""#, "Malformed node.max_attempts"),
        (r#""timeout_seconds":1.5"#, "Malformed node.timeout_seconds"),
        (r#""optional":"x""#, "Malformed node.optional"),
        (r#""call_condition":5"#, "Malformed node.call_condition"),
        (r#""workflow_name":5"#, "Malformed node.workflow_name"),
        (r#""session_mode":5"#, "Malformed node.session_mode"),
    ] {
        assert_eq!(refused(&end_with(fields)), reason, "{fields}");
    }
}

#[test]
fn start_prompt_is_checked_by_migration_first() {
    assert_eq!(
        refused(&start_with(r#""prompt":5"#)),
        "Start prompt must be a string"
    );
}

#[test]
fn validate_ignores_malformed_workflow_id() {
    for id in ["123", "null", "[]", r#""""#] {
        let definition = accepted(&top(&format!(r#""workflow_id":{id}"#)));
        assert!(definition.contains_key("workflow_id"), "{id}");
    }
    assert_eq!(
        accepted(&top(r#""workflow_id":123"#)).get("workflow_id"),
        Some(&Json::int(123))
    );
}

#[test]
fn validate_ignores_overwritten_execution_contract() {
    for contract in ["null", "5", r#""other""#] {
        assert!(
            run(&json(&top(&format!(r#""execution_contract":{contract}"#)))).is_ok(),
            "{contract}"
        );
    }
}

#[test]
fn validate_ignores_store_owned_revision_and_timestamp() {
    let input = top(r#""revision":"x","updated_at":"never""#);
    let definition = accepted(&input);
    assert_eq!(definition.get("revision"), Some(&Json::text("x")));
    assert_eq!(definition.get("updated_at"), Some(&Json::text("never")));
}

#[test]
fn context_delivery_is_qualified_as_stored() {
    let definition = accepted(&top(r#""context_delivery":"legacy""#));
    assert_eq!(
        definition.get("context_delivery"),
        Some(&Json::text("legacy"))
    );
}

#[test]
fn validate_never_refuses_name_mismatch() {
    for name in ["flow", "other name", "A.b-c_d"] {
        let input = top("").replacen("\"flow\"", &format!("\"{name}\""), 1);
        assert_eq!(accepted(&input).get("name"), Some(&Json::text(name)));
    }
}

#[test]
fn observe_repos_bounds_agree_at_seven() {
    let list = |count: usize| {
        let items: Vec<String> = (0..count).map(|_| "\"/repo/a\"".to_string()).collect();
        with_agent(&format!(r#""observe_repos":[{}]"#, items.join(",")))
    };
    assert!(run(&json(&list(7))).is_ok());
    assert_eq!(
        refused(&list(8)),
        "observe_repos must be a list of at most seven absolute paths"
    );
}

#[test]
fn qualification_applies_to_on_demand_nodes_and_orchestrator() {
    let od = top(r#""on_demand_nodes":[{"id":"od","instructions":"x"}]"#);
    assert!(run(&json(&od)).is_ok());
    let refused_orchestrator = top(r#""orchestrator":{"backend":"codex","model":5}"#);
    assert_eq!(
        refused(&refused_orchestrator),
        "Model must be a nonempty string"
    );
}
