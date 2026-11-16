//! Connected nodes: shared fields, agent nodes, Run workflow nodes and parallel fields.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use hf_domain::workflow::json::{Json, JsonObject};

fn node(fields: &str) -> JsonObject {
    let definition = accepted(&agent(fields));
    at(&definition, "nodes.1")
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

fn refused_node(fields: &str) -> String {
    refused(&agent(fields))
}

fn refused_with(node_json: &str) -> String {
    refused(&node_flow("", node_json))
}

#[test]
fn identifier_and_type_rules() {
    let identifier =
        "Identifiers must contain 1\u{2013}100 letters, digits, dots, dashes or underscores";
    for bad in [
        r#""id":"""#,
        r#""id":"-a""#,
        r#""id":5"#,
        r#""id":null"#,
        r#""id":"a b""#,
    ] {
        assert_eq!(
            refused_with(&format!(r#"{{{bad},"type":"agent"}}"#)),
            identifier,
            "{bad}"
        );
    }
    assert_eq!(refused_with(r#"{"type":"agent"}"#), identifier);
    assert_eq!(
        refused_with(r#"{"id":"s","type":"agent"}"#),
        "Duplicate node s"
    );
    assert_eq!(
        refused_with(r#"{"id":"a","type":"robot"}"#),
        "Invalid node type: a"
    );
    assert_eq!(
        refused_with(r#"{"id":"a","type":5}"#),
        "Invalid node type: a"
    );
    assert_eq!(refused_with(r#"{"id":"a"}"#), "Invalid node type: a");
}

#[test]
fn default_title_and_position() {
    let definition = accepted(&agent(""));
    assert_eq!(at(&definition, "nodes.0.position.x"), Some(&Json::int(80)));
    assert_eq!(at(&definition, "nodes.1.position.x"), Some(&Json::int(340)));
    assert_eq!(at(&definition, "nodes.2.position.x"), Some(&Json::int(600)));
    assert_eq!(at(&definition, "nodes.1.position.y"), Some(&Json::int(80)));
    assert_eq!(at(&definition, "nodes.1.title"), Some(&Json::text("a")));
}

#[test]
fn title_and_position_rules() {
    let message = "Invalid title or canvas position (x and y must be finite and nonnegative): a";
    for bad in [
        r#""title":5"#,
        r#""title":null"#,
        r#""position":null"#,
        r#""position":[]"#,
        r#""position":{"x":1}"#,
        r#""position":{"x":-1,"y":0}"#,
        r#""position":{"x":true,"y":0}"#,
        r#""position":{"x":"1","y":0}"#,
        r#""position":{"x":1,"y":NaN}"#,
        r#""position":{"x":Infinity,"y":0}"#,
    ] {
        assert_eq!(refused_node(bad), message, "{bad}");
    }
    let kept = node(r#""title":"T","position":{"x":0,"y":12.5}"#);
    assert_eq!(kept.get("title"), Some(&Json::text("T")));
    assert_eq!(
        kept.get("position")
            .and_then(Json::as_object)
            .unwrap()
            .get("y"),
        Some(&Json::Float(12.5))
    );
}

#[test]
fn position_huge_integer_refused_not_crashed() {
    let huge = "9".repeat(400);
    let message = "Invalid title or canvas position (x and y must be finite and nonnegative): a";
    assert_eq!(
        refused_node(&format!(r#""position":{{"x":{huge},"y":0}}"#)),
        message
    );
    assert_eq!(
        refused_node(&format!(r#""position":{{"x":0,"y":-{huge}}}"#)),
        message
    );
}

#[test]
fn purpose_rules() {
    let message = "Node purpose must be a nonempty single line of at most 500 characters: a";
    for bad in [
        r#""purpose":"""#,
        r#""purpose":"  ""#,
        r#""purpose":"a\nb""#,
        r#""purpose":"a\rb""#,
        r#""purpose":5"#,
        r#""purpose":null"#,
    ] {
        assert_eq!(refused_node(bad), message, "{bad}");
    }
    let five_hundred = "é".repeat(500);
    assert!(node(&format!(r#""purpose":"{five_hundred}""#)).contains_key("purpose"));
    assert_eq!(
        refused_node(&format!(r#""purpose":"{five_hundred}x""#)),
        message
    );
}

#[test]
fn always_route_and_branch_mode() {
    assert_eq!(
        refused_node(r#""always_route":"yes""#),
        "Node always_route must be a boolean"
    );
    assert_eq!(
        node(r#""always_route":true"#).get("always_route"),
        Some(&Json::Bool(true))
    );
    assert_eq!(
        refused_node(r#""branch_mode":"first""#),
        "Invalid branching mode: a"
    );
    assert_eq!(
        refused_node(r#""branch_mode":null"#),
        "Invalid branching mode: a"
    );
    for mode in ["auto", "choose_one", "all_matching"] {
        let kept = node(&format!(r#""branch_mode":"{mode}""#));
        assert_eq!(
            kept.get("branch_mode"),
            Some(&Json::text("auto")),
            "legacy routing resets {mode}"
        );
    }
    let explicit = accepted(&explicit_flow(
        r#"{"id":"a","type":"agent","branch_mode":"all_matching"}"#,
    ));
    assert_eq!(
        at(&explicit, "nodes.1.branch_mode"),
        Some(&Json::text("choose_one"))
    );
}

#[test]
fn agent_scalar_defaults() {
    let defaults = node("");
    for (key, value) in [
        ("optional", Json::Bool(false)),
        ("role", Json::text("task")),
        ("instructions", Json::text("")),
        ("freedom", Json::text("publish")),
        ("execution_mode", Json::text("headless")),
        ("session_mode", Json::text("agent_decides")),
        ("max_attempts", Json::int(3)),
        ("max_context_questions", Json::int(10)),
    ] {
        assert_eq!(defaults.get(key), Some(&value), "{key}");
    }
    assert!(!defaults.contains_key("network"));
    assert!(!defaults.contains_key("timeout_seconds"));
}

#[test]
fn role_decides_default_freedom() {
    for (role, freedom) in [
        ("planning", "read_only"),
        ("review", "read_only"),
        ("implementation", "write_in_repo"),
        ("task", "publish"),
    ] {
        let found = node(&format!(r#""role":"{role}""#));
        assert_eq!(found.get("freedom"), Some(&Json::text(freedom)), "{role}");
    }
    assert_eq!(refused_node(r#""role":"boss""#), "Invalid agent role: a");
    assert_eq!(refused_node(r#""role":null"#), "Invalid agent role: a");
}

#[test]
fn planning_flags_default_true() {
    let planning = node(r#""role":"planning""#);
    assert_eq!(
        planning.get("require_technical_plan"),
        Some(&Json::Bool(true))
    );
    assert_eq!(planning.get("require_tasks"), Some(&Json::Bool(true)));
    assert!(!node(r#""role":"review""#).contains_key("require_tasks"));
    assert_eq!(
        refused_node(r#""role":"planning","require_tasks":1"#),
        "Planning require_tasks must be a boolean"
    );
    assert_eq!(
        refused_node(r#""role":"planning","require_technical_plan":null"#),
        "Planning require_technical_plan must be a boolean"
    );
}

#[test]
fn freedom_rules() {
    let message = "Workflow nodes support read_only, write_in_repo, publish or unrestricted";
    assert_eq!(refused_node(r#""freedom":"root""#), message);
    assert_eq!(refused_node(r#""freedom":null"#), message);
    assert_eq!(
        refused_node(r#""role":"implementation","freedom":"read_only""#),
        "Implementation nodes cannot use read_only"
    );
    assert_eq!(
        node(r#""role":"review","freedom":"unrestricted""#).get("freedom"),
        Some(&Json::text("unrestricted"))
    );
}

#[test]
fn instructions_and_network() {
    let message = "Invalid instructions or network: a";
    assert_eq!(refused_node(r#""instructions":5"#), message);
    assert_eq!(refused_node(r#""instructions":null"#), message);
    for bad in ["0", "1", "1.0", r#""yes""#, "[]"] {
        assert_eq!(
            refused_node(&format!(r#""network":{bad}"#)),
            message,
            "network {bad}"
        );
    }
    assert_eq!(
        node(r#""network":true"#).get("network"),
        Some(&Json::Bool(true))
    );
    assert_eq!(node(r#""network":null"#).get("network"), Some(&Json::Null));
}

#[test]
fn execution_and_session_modes() {
    assert_eq!(
        refused_node(r#""execution_mode":"batch""#),
        "Invalid execution mode: a"
    );
    assert_eq!(
        refused_node(r#""execution_mode":null"#),
        "Invalid execution mode: a"
    );
    assert_eq!(
        node(r#""execution_mode":"prefer_subagent""#).get("execution_mode"),
        Some(&Json::text("prefer_subagent"))
    );
    assert_eq!(
        refused_node(r#""session_mode":"later""#),
        "Invalid session mode: a"
    );
    for mode in ["resume", "fresh", "agent_decides", "continue_previous"] {
        assert_eq!(
            node(&format!(r#""session_mode":"{mode}""#)).get("session_mode"),
            Some(&Json::text(mode))
        );
    }
}

#[test]
fn attempt_timeout_and_question_limits() {
    for (key, maximum) in [("max_attempts", 10000), ("max_context_questions", 100)] {
        assert_eq!(
            node(&format!(r#""{key}":{maximum}"#)).get(key),
            Some(&Json::int(maximum))
        );
        let message = format!("{key} must be an integer between 1 and {maximum}");
        for bad in [
            "0".to_string(),
            (maximum + 1).to_string(),
            "true".to_string(),
            "2.5".to_string(),
        ] {
            assert_eq!(
                refused_node(&format!(r#""{key}":{bad}"#)),
                message,
                "{key} {bad}"
            );
        }
    }
    let message = "timeout_seconds must be a nonnegative integer";
    for bad in ["-1", "true", "1.5", r#""5""#] {
        assert_eq!(
            refused_node(&format!(r#""timeout_seconds":{bad}"#)),
            message,
            "{bad}"
        );
    }
    for ok in ["0", "3600", "null"] {
        assert!(node(&format!(r#""timeout_seconds":{ok}"#)).contains_key("timeout_seconds"));
    }
}

#[test]
fn optional_flag_is_boolean() {
    assert_eq!(
        refused_node(r#""optional":"yes""#),
        "Agent optional must be a boolean"
    );
    assert_eq!(
        refused_node(r#""optional":null"#),
        "Agent optional must be a boolean"
    );
    assert_eq!(
        node(r#""optional":false"#).get("optional"),
        Some(&Json::Bool(false))
    );
    // An optional node alone on the only path is unsafe; the graph tests cover the safe form.
    assert!(
        refused_node(r#""optional":true"#)
            .starts_with("Optional nodes need a safe parallel branch")
    );
}

#[test]
fn node_git_preparation_rules() {
    assert_eq!(
        refused_node(r#""branch":"feature/x""#),
        "a: branch and base are set together; pass both, for example branch='feature/x' and base='origin/develop'"
    );
    assert_eq!(
        refused_node(r#""fetch":"origin""#),
        "a: fetch must be true or a list of remote names"
    );
    assert_eq!(
        refused_node(r#""branch":"x","base":"origin/main","freedom":"read_only""#),
        "a: branch preparation is unavailable at read_only; use write_in_repo or above, or pass fetch without branch"
    );
    assert!(
        node(r#""branch":"feature/x","base":"origin/main","fetch":["origin"]"#)
            .contains_key("branch")
    );
}
