//! On-demand nodes and resource declarations.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use hf_domain::workflow::json::{Json, JsonObject};

fn on_demand(nodes: &str) -> String {
    top(&format!(r#""on_demand_nodes":[{nodes}]"#))
}

fn od_node(fields: &str) -> JsonObject {
    let tail = if fields.is_empty() {
        String::new()
    } else {
        format!(",{fields}")
    };
    let definition = accepted(&on_demand(&format!(
        r#"{{"id":"od","instructions":"do it"{tail}}}"#
    )));
    at(&definition, "on_demand_nodes.0")
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

fn refused_od(fields: &str) -> String {
    let tail = if fields.is_empty() {
        String::new()
    } else {
        format!(",{fields}")
    };
    refused(&on_demand(&format!(
        r#"{{"id":"od","instructions":"do it"{tail}}}"#
    )))
}

#[test]
fn defaults_and_key_order() {
    let node = od_node("");
    let order: Vec<&str> = node.keys().collect();
    assert_eq!(
        order,
        [
            "id",
            "instructions",
            "title",
            "call_condition",
            "agent",
            "freedom",
            "session_mode",
            "execution_mode"
        ]
    );
    assert_eq!(node.get("title"), Some(&Json::text("od")));
    assert_eq!(node.get("freedom"), Some(&Json::text("read_only")));
    assert_eq!(node.get("session_mode"), Some(&Json::text("agent_decides")));
    assert_eq!(node.get("execution_mode"), Some(&Json::text("headless")));
    assert_eq!(
        node.get("agent")
            .and_then(Json::as_object)
            .unwrap()
            .get("backend"),
        Some(&Json::text("codex"))
    );
}

#[test]
fn list_and_key_rules() {
    assert_eq!(
        refused(&top(r#""on_demand_nodes":{}"#)),
        "on_demand_nodes must be an array"
    );
    assert_eq!(
        refused(&top(r#""on_demand_nodes":null"#)),
        "on_demand_nodes must be an array"
    );
    let keys = "On-demand nodes accept only id, title, instructions, call_condition, agent, freedom, network, session_mode, execution_mode and resources";
    assert_eq!(refused(&on_demand("5")), keys);
    assert_eq!(refused_od(r#""role":"task""#), keys);
}

#[test]
fn identifier_and_duplicates() {
    let identifier =
        "Identifiers must contain 1\u{2013}100 letters, digits, dots, dashes or underscores";
    assert_eq!(refused(&on_demand(r#"{"instructions":"x"}"#)), identifier);
    assert_eq!(
        refused(&on_demand(r#"{"id":"-","instructions":"x"}"#)),
        identifier
    );
    assert_eq!(
        refused(&on_demand(r#"{"id":"s","instructions":"x"}"#)),
        "Duplicate node s"
    );
    let twice = r#"{"id":"od","instructions":"x"},{"id":"od","instructions":"y"}"#;
    assert_eq!(refused(&on_demand(twice)), "Duplicate node od");
}

#[test]
fn text_fields() {
    let message = "On-demand instructions must be nonempty text: od";
    assert_eq!(refused(&on_demand(r#"{"id":"od"}"#)), message);
    for bad in ["5", "null", r#""""#, r#""  ""#] {
        assert_eq!(
            refused(&on_demand(&format!(
                r#"{{"id":"od","instructions":{bad}}}"#
            ))),
            message,
            "{bad}"
        );
    }
    let message = "On-demand title and call_condition must be text: od";
    assert_eq!(refused_od(r#""title":5"#), message);
    assert_eq!(refused_od(r#""call_condition":null"#), message);
    assert_eq!(
        od_node(r#""title":"T","call_condition":"when""#).get("call_condition"),
        Some(&Json::text("when"))
    );
}

#[test]
fn agent_is_a_grant_candidate() {
    assert_eq!(refused_od(r#""agent":null"#), "Agent must be an object");
    let kept = od_node(r#""agent":{"backend":"claude","observe_repos":["/repo/a"]}"#);
    let agent = kept.get("agent").and_then(Json::as_object).unwrap();
    assert_eq!(
        agent.get("observe_repos"),
        Some(&Json::texts(&["/repo/a".to_string()]))
    );
    assert_eq!(
        refused_od(r#""agent":{"backend":"claude","allowed_commands":["git status"]}"#),
        "Node od claude candidate: allowed_commands pattern 'git status' refused: allowed_commands is supported only at write_in_repo and publish, not read_only"
    );
}

#[test]
fn freedom_and_network() {
    let message = "Invalid on-demand freedom or network: od";
    assert_eq!(refused_od(r#""freedom":"root""#), message);
    assert_eq!(refused_od(r#""freedom":null"#), message);
    assert_eq!(
        od_node(r#""freedom":"publish""#).get("freedom"),
        Some(&Json::text("publish"))
    );
    for bad in ["0", "1", "1.0", r#""true""#] {
        assert_eq!(
            refused_od(&format!(r#""network":{bad}"#)),
            message,
            "network {bad}"
        );
    }
    assert_eq!(
        od_node(r#""network":false"#).get("network"),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        od_node(r#""network":null"#).get("network"),
        Some(&Json::Null)
    );
}

#[test]
fn on_demand_network_strict() {
    // Unlike a connected node, an on-demand node reports a freedom/network message.
    assert_eq!(
        refused_od(r#""network":1"#),
        "Invalid on-demand freedom or network: od"
    );
    assert!(od_node(r#""network":true"#).contains_key("network"));
}

#[test]
fn modes() {
    assert_eq!(
        refused_od(r#""session_mode":"continue_previous""#),
        "Invalid on-demand session_mode: od"
    );
    assert_eq!(
        refused_od(r#""session_mode":null"#),
        "Invalid on-demand session_mode: od"
    );
    for mode in ["agent_decides", "fresh", "resume"] {
        assert!(od_node(&format!(r#""session_mode":"{mode}""#)).contains_key("session_mode"));
    }
    assert_eq!(
        refused_od(r#""execution_mode":"batch""#),
        "Invalid on-demand execution_mode: od"
    );
    assert_eq!(
        od_node(r#""execution_mode":"prefer_subagent""#).get("execution_mode"),
        Some(&Json::text("prefer_subagent"))
    );
}
