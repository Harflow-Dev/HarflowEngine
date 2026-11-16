//! Run workflow nodes, and the fields of parallel boundary nodes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use hf_domain::workflow::json::{Json, JsonObject};

fn run_node(extra: &str) -> String {
    let tail = if extra.is_empty() {
        String::new()
    } else {
        format!(",{extra}")
    };
    format!(r#"{{"id":"a","type":"workflow","workflow_ref":{{"workflow_id":"w1"}}{tail}}}"#)
}

fn accepted_run(extra: &str) -> JsonObject {
    let definition = accepted(&explicit_flow(&run_node(extra)));
    at(&definition, "nodes.1")
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

fn refused_run(extra: &str) -> String {
    refused(&explicit_flow(&run_node(extra)))
}

#[test]
fn defaults_and_key_order() {
    let node = accepted_run("");
    let order: Vec<&str> = node.keys().collect();
    assert_eq!(
        order,
        [
            "id",
            "type",
            "workflow_ref",
            "title",
            "position",
            "branch_mode",
            "workflow_name",
            "orchestrator_mode",
            "child_session_policy",
            "instructions",
            "optional",
            "max_attempts",
        ]
    );
    assert_eq!(node.get("orchestrator_mode"), Some(&Json::text("child")));
    assert_eq!(
        node.get("child_session_policy"),
        Some(&Json::text("agent_decides"))
    );
    assert_eq!(node.get("max_attempts"), Some(&Json::int(3)));
    assert_eq!(node.get("branch_mode"), Some(&Json::text("choose_one")));
}

#[test]
fn needs_explicit_routing() {
    assert_eq!(
        refused(&node_flow("", &run_node(""))),
        "Run workflow nodes require explicit routing_mode: a"
    );
}

#[test]
fn rejected_agent_fields() {
    for field in [
        "agent",
        "role",
        "freedom",
        "network",
        "session_mode",
        "execution_mode",
        "max_context_questions",
    ] {
        assert_eq!(
            refused_run(&format!(r#""{field}":null"#)),
            format!("Run workflow nodes do not accept {field}: a"),
            "{field}"
        );
    }
}

#[test]
fn reference_must_name_a_workflow() {
    let message = "Run workflow node requires workflow_ref.workflow_id: a";
    for bad in [
        "null",
        "{}",
        r#"{"workflow_id":""}"#,
        r#"{"workflow_id":"  "}"#,
        r#"{"workflow_id":5}"#,
        r#"{"workflow_id":"w1","other":1}"#,
        r#""w1""#,
    ] {
        let node = format!(r#"{{"id":"a","type":"workflow","workflow_ref":{bad}}}"#);
        assert_eq!(refused(&explicit_flow(&node)), message, "{bad}");
    }
    let node = r#"{"id":"a","type":"workflow"}"#;
    assert_eq!(refused(&explicit_flow(node)), message);
}

#[test]
fn text_and_choice_fields() {
    assert_eq!(
        refused_run(r#""workflow_name":5"#),
        "Run workflow workflow_name must be text: a"
    );
    assert_eq!(
        accepted_run(r#""workflow_name":"child""#).get("workflow_name"),
        Some(&Json::text("child"))
    );
    assert_eq!(
        refused_run(r#""orchestrator_mode":"parent""#),
        "Run workflow orchestrator_mode must be child or current: a"
    );
    assert_eq!(
        accepted_run(r#""orchestrator_mode":"current""#).get("orchestrator_mode"),
        Some(&Json::text("current"))
    );
    assert_eq!(
        refused_run(r#""child_session_policy":"later""#),
        "Invalid child_session_policy: a"
    );
    for policy in ["fresh", "resume", "agent_decides"] {
        assert!(
            accepted_run(&format!(r#""child_session_policy":"{policy}""#))
                .contains_key("workflow_ref")
        );
    }
    assert_eq!(
        refused_run(r#""instructions":null"#),
        "Run workflow instructions must be text: a"
    );
    assert_eq!(
        refused_run(r#""optional":1"#),
        "Run workflow optional must be a boolean: a"
    );
}

#[test]
fn attempts_and_timeout() {
    assert_eq!(
        refused_run(r#""max_attempts":0"#),
        "max_attempts must be an integer between 1 and 10000"
    );
    assert_eq!(
        accepted_run(r#""max_attempts":10000"#).get("max_attempts"),
        Some(&Json::int(10000))
    );
    assert_eq!(
        refused_run(r#""timeout_seconds":-1"#),
        "timeout_seconds must be a nonnegative integer: a"
    );
    assert_eq!(
        refused_run(r#""timeout_seconds":false"#),
        "timeout_seconds must be a nonnegative integer: a"
    );
    assert!(accepted_run(r#""timeout_seconds":0"#).contains_key("timeout_seconds"));
}

fn parallel_flow(start_fields: &str) -> String {
    let tail = if start_fields.is_empty() {
        String::new()
    } else {
        format!(",{start_fields}")
    };
    format!(
        r#"{{"name":"flow","routing_mode":"explicit","nodes":[{{"id":"s","type":"start"}},{{"id":"p","type":"parallel_start","parallel_group_id":"g"{tail}}},{{"id":"x","type":"agent"}},{{"id":"y","type":"agent"}},{{"id":"q","type":"parallel_end","parallel_group_id":"g"}},{{"id":"e","type":"end"}}],"connections":[{{"id":"c1","source":"s","target":"p"}},{{"id":"c2","source":"p","target":"x"}},{{"id":"c3","source":"p","target":"y"}},{{"id":"c4","source":"x","target":"q"}},{{"id":"c5","source":"y","target":"q"}},{{"id":"c6","source":"q","target":"e"}}]}}"#
    )
}

#[test]
fn parallel_start_defaults() {
    let definition = accepted(&parallel_flow(""));
    assert_eq!(
        at(&definition, "nodes.1.branch_selection"),
        Some(&Json::text("all"))
    );
    assert_eq!(
        at(&definition, "nodes.1.selection_guidance"),
        Some(&Json::text(""))
    );
    assert_eq!(
        at(&definition, "nodes.1.branch_mode"),
        Some(&Json::text("auto"))
    );
    assert_eq!(
        at(&definition, "nodes.4.branch_mode"),
        Some(&Json::text("choose_one"))
    );
    let chosen = accepted(&parallel_flow(
        r#""branch_selection":"orchestrator","selection_guidance":"pick""#,
    ));
    assert_eq!(
        at(&chosen, "nodes.1.branch_selection"),
        Some(&Json::text("orchestrator"))
    );
}

#[test]
fn parallel_start_field_rules() {
    assert_eq!(
        refused(&parallel_flow(r#""branch_selection":"some""#)),
        "Parallel branch_selection must be all or orchestrator"
    );
    assert_eq!(
        refused(&parallel_flow(r#""branch_selection":null"#)),
        "Parallel branch_selection must be all or orchestrator"
    );
    assert_eq!(
        refused(&parallel_flow(r#""selection_guidance":5"#)),
        "Parallel selection_guidance must be a string"
    );
}

#[test]
fn parallel_nodes_need_explicit_routing() {
    let legacy = parallel_flow("").replace(r#""routing_mode":"explicit","#, "");
    assert_eq!(
        refused(&legacy),
        "Parallel boundaries require explicit routing_mode"
    );
}
