//! The top of a definition: name, defaults, limits, orchestrator and the start prompt.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use hf_domain::workflow::json::Json;

#[test]
fn output_key_order() {
    let definition = accepted(&top(""));
    let order: Vec<&str> = definition.keys().collect();
    assert_eq!(
        order,
        [
            "name",
            "nodes",
            "connections",
            "orchestrator",
            "on_demand_nodes",
            "max_on_demand_calls",
            "schema_version",
            "description",
            "max_parallel",
            "max_transitions",
            "max_decision_attempts",
            "max_inspections",
        ]
    );
    assert_eq!(definition.get("schema_version"), Some(&Json::int(1)));
}

#[test]
fn defaults_are_applied() {
    let definition = accepted(&top(""));
    for (key, value) in [
        ("max_on_demand_calls", 5),
        ("max_parallel", 4),
        ("max_transitions", 100),
        ("max_decision_attempts", 3),
        ("max_inspections", 20),
    ] {
        assert_eq!(definition.get(key), Some(&Json::int(value)), "{key}");
    }
    assert_eq!(definition.get("description"), Some(&Json::text("")));
    assert_eq!(
        definition.get("on_demand_nodes"),
        Some(&Json::Array(Vec::new()))
    );
    assert_eq!(definition.get("context_delivery"), None);
    assert_eq!(definition.get("routing_mode"), None);
}

#[test]
fn unknown_keys_survive() {
    let input = r#"{"name":"flow","extra":{"k":[1]},"nodes":[{"id":"s","type":"start","note":"x"},{"id":"e","type":"end"}],"connections":[{"id":"c1","source":"s","target":"e","tag":true}]}"#;
    let definition = accepted(input);
    assert_eq!(at(&definition, "extra.k.0"), Some(&Json::int(1)));
    assert_eq!(at(&definition, "nodes.0.note"), Some(&Json::text("x")));
    assert_eq!(
        at(&definition, "connections.0.tag"),
        Some(&Json::Bool(true))
    );
}

#[test]
fn name_rules() {
    for good in ["a", "A1", "my flow", "a.b-c_d", &"a".repeat(100)] {
        let input = format!(
            r#"{{"name":{good:?},"nodes":[{{"id":"s","type":"start"}},{{"id":"e","type":"end"}}],"connections":[{{"id":"c","source":"s","target":"e"}}]}}"#
        );
        assert!(run(&json(&input)).is_ok(), "{good}");
    }
    for bad in ["", " a", "a ", "-a", "a/b", "é", "a\\n", &"a".repeat(101)] {
        let input = format!(r#"{{"name":"{bad}","nodes":[],"connections":[]}}"#);
        let message = run(&json(&input)).unwrap_err();
        assert!(
            message.starts_with("Workflow names must contain 1\u{2013}100"),
            "{bad:?}: {message}"
        );
    }
    assert!(refused(r#"{"nodes":[],"connections":[]}"#).starts_with("Workflow names"));
    assert_eq!(refused("[]"), "Workflow definition must be an object");
}

#[test]
fn context_delivery_and_routing_mode() {
    assert_eq!(
        accepted(&top(r#""context_delivery":"legacy""#)).get("context_delivery"),
        Some(&Json::text("legacy"))
    );
    assert!(run(&json(&top(r#""context_delivery":"optimized_v1""#))).is_ok());
    for bad in ["null", "5", r#""other""#, "[]"] {
        assert_eq!(
            refused(&top(&format!(r#""context_delivery":{bad}"#))),
            "context_delivery must be legacy or optimized_v1",
            "{bad}"
        );
    }
    for bad in ["5", r#""legacy""#, r#""""#, "[]"] {
        assert_eq!(
            refused(&top(&format!(r#""routing_mode":{bad}"#))),
            "Invalid routing_mode",
            "{bad}"
        );
    }
}

#[test]
fn description_must_be_text() {
    assert_eq!(
        accepted(&top(r#""description":"d""#)).get("description"),
        Some(&Json::text("d"))
    );
    for bad in ["null", "5", "[]"] {
        assert_eq!(
            refused(&top(&format!(r#""description":{bad}"#))),
            "Description must be text",
            "{bad}"
        );
    }
}

#[test]
fn limit_bounds() {
    for (key, maximum) in [
        ("max_on_demand_calls", 50),
        ("max_parallel", 64),
        ("max_transitions", 10000),
        ("max_decision_attempts", 10),
        ("max_inspections", 1000),
    ] {
        for ok in [1, maximum] {
            let definition = accepted(&top(&format!(r#""{key}":{ok}"#)));
            assert_eq!(definition.get(key), Some(&Json::int(ok)));
        }
        let message = format!("{key} must be an integer between 1 and {maximum}");
        for bad in [
            "0".to_string(),
            "-1".to_string(),
            (maximum + 1).to_string(),
            "true".to_string(),
            "1.0".to_string(),
            "\"1\"".to_string(),
            "null".to_string(),
        ] {
            assert_eq!(
                refused(&top(&format!(r#""{key}":{bad}"#))),
                message,
                "{key} {bad}"
            );
        }
    }
}

#[test]
fn orchestrator_rules() {
    assert_eq!(
        refused(&top(r#""orchestrator":null"#)),
        "Orchestrator must be an agent object"
    );
    assert_eq!(
        refused(&top(r#""orchestrator":[]"#)),
        "Orchestrator must be an agent object"
    );
    assert_eq!(
        refused(&top(
            r#""orchestrator":{"backend":"codex","instructions":5}"#
        )),
        "Orchestrator instructions must be text"
    );
    let definition = accepted(&top(
        r#""orchestrator":{"instructions":"go","backend":"claude"}"#,
    ));
    let orchestrator = at(&definition, "orchestrator").unwrap();
    let order = keys(orchestrator);
    assert_eq!(
        order,
        [
            "backend",
            "caller_approvals",
            "max_turns",
            "fallbacks",
            "instructions"
        ]
    );
    assert_eq!(
        at(&definition, "orchestrator.instructions"),
        Some(&Json::text("go"))
    );
}

#[test]
fn start_prompt_moves_to_instructions() {
    let input = r#"{"name":"flow","nodes":[{"id":"s","type":"start","prompt":"build it"},{"id":"e","type":"end"}],"connections":[{"id":"c","source":"s","target":"e"}]}"#;
    let definition = accepted(input);
    assert_eq!(
        at(&definition, "orchestrator.instructions"),
        Some(&Json::text("build it"))
    );
    assert_eq!(at(&definition, "nodes.0.prompt"), None);
    let with_instructions = input.replace(
        r#""nodes""#,
        r#""orchestrator":{"backend":"codex","instructions":"own"},"nodes""#,
    );
    assert_eq!(
        at(&accepted(&with_instructions), "orchestrator.instructions"),
        Some(&Json::text("own"))
    );
    let bad = input.replace(r#""build it""#, "5");
    assert_eq!(refused(&bad), "Start prompt must be a string");
}

#[test]
fn nodes_and_connections_shape() {
    let message = "Workflow requires nodes and connections arrays";
    assert_eq!(refused(r#"{"name":"flow"}"#), message);
    assert_eq!(
        refused(r#"{"name":"flow","nodes":[],"connections":[]}"#),
        message
    );
    assert_eq!(
        refused(r#"{"name":"flow","nodes":{},"connections":[]}"#),
        message
    );
    assert_eq!(
        refused(r#"{"name":"flow","nodes":[{"id":"s","type":"start"}],"connections":{}}"#),
        message
    );
    assert_eq!(
        refused(r#"{"name":"flow","nodes":[5],"connections":[]}"#),
        "Every node must be an object"
    );
}
