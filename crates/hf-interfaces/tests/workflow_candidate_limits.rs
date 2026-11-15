//! Candidate grants (observed repositories, command patterns) and the scalar limits,
//! including explicit null values.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use hf_domain::workflow::json::{Json, JsonObject};

fn node_agent(settings: &str) -> JsonObject {
    let definition = accepted(&with_agent(settings));
    at(&definition, "nodes.1.agent")
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

fn refused_agent(settings: &str) -> String {
    refused(&with_agent(settings))
}

fn paths(agent: &JsonObject) -> Vec<String> {
    let Some(Json::Array(items)) = agent.get("observe_repos") else {
        panic!("observe_repos missing");
    };
    items
        .iter()
        .map(|item| item.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn observe_repos_resolved_and_deduplicated() {
    let resolver = MapResolver::new([
        ("/repo/a".to_string(), Ok("/real/x".to_string())),
        ("/repo/b".to_string(), Ok("/real/x".to_string())),
        ("/repo/c".to_string(), Ok("/real/y".to_string())),
    ]);
    let input = json(&with_agent(
        r#""observe_repos":["/repo/a","/repo/c","/repo/b"]"#,
    ));
    let definition = run_with(&resolver, &input).unwrap();
    let agent = at(&definition, "nodes.1.agent")
        .unwrap()
        .as_object()
        .unwrap();
    assert_eq!(paths(agent), ["/real/x", "/real/y"]);
}

#[test]
fn observe_repos_rules() {
    let bounds = "observe_repos must be a list of at most seven absolute paths";
    assert_eq!(refused_agent(r#""observe_repos":"/repo/a""#), bounds);
    assert_eq!(refused_agent(r#""observe_repos":{"/repo/a":1}"#), bounds);
    let eight = ["/repo/a"; 8].map(|path| format!("\"{path}\"")).join(",");
    assert_eq!(
        refused_agent(&format!(r#""observe_repos":[{eight}]"#)),
        bounds
    );
    let seven = ["/repo/a"; 7].map(|path| format!("\"{path}\"")).join(",");
    assert_eq!(
        paths(&node_agent(&format!(r#""observe_repos":[{seven}]"#))),
        ["/repo/a"]
    );
    let absolute = "observe_repos must contain absolute paths";
    assert_eq!(refused_agent(r#""observe_repos":["repo/a"]"#), absolute);
    assert_eq!(refused_agent(r#""observe_repos":[5]"#), absolute);
    assert_eq!(refused_agent(r#""observe_repos":["/repo/a",""]"#), absolute);
    assert_eq!(
        refused_agent(r#""observe_repos":["/missing"]"#),
        "unmapped path /missing"
    );
    assert_eq!(
        refused(&top(
            r#""orchestrator":{"backend":"codex","observe_repos":[]}"#
        )),
        "observe_repos belongs to workflow agent nodes"
    );
}

#[test]
fn resolver_refusal_text_is_the_message() {
    let resolver =
        MapResolver::new([("/repo/a".to_string(), Err("embedded null byte".to_string()))]);
    let input = json(&with_agent(r#""observe_repos":["/repo/a"]"#));
    assert_eq!(
        run_with(&resolver, &input).unwrap_err(),
        "embedded null byte"
    );
}

#[test]
fn null_observe_repos_empty_list() {
    let agent = node_agent(r#""observe_repos":null"#);
    assert_eq!(agent.get("observe_repos"), Some(&Json::Array(Vec::new())));
    assert_eq!(
        paths(&node_agent(r#""observe_repos":[]"#)),
        Vec::<String>::new()
    );
}

#[test]
fn allowed_commands_rules() {
    assert_eq!(
        refused(&top(
            r#""orchestrator":{"backend":"codex","allowed_commands":[]}"#
        )),
        "allowed_commands belongs to workflow agent nodes, not orchestrators or builders"
    );
    assert_eq!(
        refused_agent(r#""allowed_commands":"git""#),
        "allowed_commands must be a list of strings"
    );
    assert_eq!(
        refused_agent(r#""allowed_commands":[""]"#),
        "allowed_commands entries must not be empty"
    );
    assert_eq!(
        refused_agent(r#""allowed_commands":[5]"#),
        "allowed_commands entries must be strings"
    );
    assert_eq!(
        refused_agent(r#""allowed_commands":["x","x"]"#),
        "allowed_commands pattern 'x' is duplicated"
    );
    assert!(!node_agent(r#""allowed_commands":[]"#).contains_key("allowed_commands"));
    let kept = node_agent(r#""backend":"claude","allowed_commands":["c0"]"#);
    assert_eq!(
        kept.get("allowed_commands"),
        Some(&Json::texts(&["c0".to_string()]))
    );
}

#[test]
fn allowed_commands_checked_against_node_freedom() {
    let read_only = agent(
        r#""freedom":"read_only","agent":{"backend":"claude","allowed_commands":["git status"]}"#,
    );
    assert_eq!(
        refused(&read_only),
        "Node a claude candidate: allowed_commands pattern 'git status' refused: allowed_commands is supported only at write_in_repo and publish, not read_only"
    );
    let fallback = agent(
        r#""freedom":"read_only","agent":{"backend":"codex","fallbacks":[{"backend":"claude","allowed_commands":["git status"]}]}"#,
    );
    assert!(refused(&fallback).starts_with("Node a claude candidate: allowed_commands pattern"));
}

#[test]
fn explicit_null_model_effort_idle_kept() {
    let agent =
        node_agent(r#""model":null,"reasoning_effort":null,"background_idle_seconds":null"#);
    for key in ["model", "reasoning_effort", "background_idle_seconds"] {
        assert_eq!(agent.get(key), Some(&Json::Null), "{key}");
    }
    let order: Vec<&str> = agent.keys().collect();
    assert_eq!(
        &order[..4],
        [
            "backend",
            "model",
            "reasoning_effort",
            "background_idle_seconds"
        ]
    );
}

#[test]
fn model_must_be_nonblank_text() {
    let message = "Model must be a nonempty string";
    for value in ["5", r#""""#, r#""  ""#, r#"" ""#, "[]", "true"] {
        assert_eq!(
            refused_agent(&format!(r#""model":{value}"#)),
            message,
            "{value}"
        );
    }
    assert_eq!(
        node_agent(r#""model":"gpt-x""#).get("model"),
        Some(&Json::text("gpt-x"))
    );
}

#[test]
fn background_idle_is_a_float() {
    let idle = |value: &str| node_agent(&format!(r#""background_idle_seconds":{value}"#));
    assert_eq!(
        idle("5").get("background_idle_seconds"),
        Some(&Json::Float(5.0))
    );
    assert_eq!(
        idle("21600").get("background_idle_seconds"),
        Some(&Json::Float(21600.0))
    );
    assert_eq!(
        idle("0.001").get("background_idle_seconds"),
        Some(&Json::Float(0.001))
    );
    let message = "background_idle_seconds must be a finite positive number no greater than 21600";
    for bad in [
        "0", "-1", "21600.5", "21601", "true", r#""5""#, "NaN", "Infinity", "[5]", "1e400",
    ] {
        assert_eq!(
            refused_agent(&format!(r#""background_idle_seconds":{bad}"#)),
            message,
            "{bad}"
        );
    }
}

#[test]
fn null_max_turns_defaults() {
    let capped = node_agent(r#""backend":"claude","max_turns":null"#);
    assert_eq!(capped.get("max_turns"), Some(&Json::int(100)));
    let vibe = node_agent(r#""backend":"vibe""#);
    assert_eq!(vibe.get("max_turns"), Some(&Json::int(100)));
    let uncapped = node_agent(r#""backend":"codex","max_turns":null"#);
    assert_eq!(uncapped.get("max_turns"), Some(&Json::Null));
    assert_eq!(node_agent(r#""backend":"codex""#).get("max_turns"), None);
}

#[test]
fn max_turns_bounds() {
    let turns = |value: &str| node_agent(&format!(r#""backend":"claude","max_turns":{value}"#));
    assert_eq!(turns("1").get("max_turns"), Some(&Json::int(1)));
    assert_eq!(turns("10000").get("max_turns"), Some(&Json::int(10000)));
    for bad in [
        "0",
        "-1",
        "10001",
        "true",
        "5.0",
        r#""5""#,
        "99999999999999999999999",
    ] {
        assert_eq!(
            refused_agent(&format!(r#""backend":"claude","max_turns":{bad}"#)),
            "max_turns must be an integer between 1 and 10000",
            "{bad}"
        );
    }
    assert_eq!(
        refused_agent(r#""backend":"opencode","max_turns":5"#),
        "the opencode CLI has no turn cap, so max_turns=5 cannot be honoured; omit it, or use a backend whose capabilities report supports_turn_cap"
    );
}

#[test]
fn reasoning_effort_rules() {
    for level in ["low", "medium", "high", "xhigh"] {
        let agent = node_agent(&format!(
            r#""backend":"codex","reasoning_effort":"{level}""#
        ));
        assert_eq!(agent.get("reasoning_effort"), Some(&Json::text(level)));
    }
    assert_eq!(
        refused_agent(r#""reasoning_effort":"ultra""#),
        "Invalid reasoning_effort"
    );
    assert_eq!(
        refused_agent(r#""reasoning_effort":5"#),
        "Invalid reasoning_effort"
    );
    assert_eq!(
        refused_agent(r#""backend":"antigravity","reasoning_effort":"xhigh""#),
        "the antigravity CLI does not accept reasoning_effort='xhigh'; it accepts ['low', 'medium', 'high']"
    );
    for backend in ["cursor", "vibe"] {
        let message = refused_agent(&format!(
            r#""backend":"{backend}","reasoning_effort":"low""#
        ));
        assert!(
            message.contains("has no reasoning effort control"),
            "{message}"
        );
    }
}
