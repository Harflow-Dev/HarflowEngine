//! Every defaulted field is exercised absent, null and empty.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

#[derive(Clone, Copy)]
enum Place {
    Top,
    Orchestrator,
    Node,
    Candidate,
    Claude,
    Connection,
    OnDemand,
}

const OK: &str = "ok";
const POSITION: &str =
    "Invalid title or canvas position (x and y must be finite and nonnegative): a";
const PURPOSE: &str = "Node purpose must be a nonempty single line of at most 500 characters: a";
const IDLE: &str = "background_idle_seconds must be a finite positive number no greater than 21600";

fn build(place: Place, field: &str, value: Option<&str>) -> String {
    let fragment = value
        .map(|text| format!(r#""{field}":{text}"#))
        .unwrap_or_default();
    let tail = if fragment.is_empty() {
        String::new()
    } else {
        format!(",{fragment}")
    };
    match place {
        Place::Top => top(&fragment),
        Place::Orchestrator => top(&format!(r#""orchestrator":{{"backend":"codex"{tail}}}"#)),
        Place::Node => agent(&fragment),
        Place::Candidate => with_agent(&fragment),
        Place::Claude => with_agent(&format!(r#""backend":"claude"{tail}"#)),
        Place::Connection => agent("").replace(
            r#"{"id":"c1","source":"s","target":"a"}"#,
            &format!(r#"{{"id":"c1","source":"s","target":"a"{tail}}}"#),
        ),
        Place::OnDemand => top(&format!(
            r#""on_demand_nodes":[{{"id":"od","instructions":"x"{tail}}}]"#
        )),
    }
}

fn outcome(input: &str) -> String {
    match run(&json(input)) {
        Ok(_) => OK.to_string(),
        Err(message) => message,
    }
}

/// `(place, field, the empty value, outcome when absent / null / empty)`.
type Row = (Place, &'static str, &'static str, [&'static str; 3]);

const ROWS: &[Row] = &[
    (
        Place::Top,
        "description",
        r#""""#,
        [OK, "Description must be text", OK],
    ),
    (
        Place::Top,
        "on_demand_nodes",
        "[]",
        [OK, "on_demand_nodes must be an array", OK],
    ),
    (
        Place::Top,
        "resources",
        "[]",
        [
            OK,
            "resource names must be a list of names",
            "request 1 to 16 resource names, got 0",
        ],
    ),
    (
        Place::Orchestrator,
        "instructions",
        r#""""#,
        [OK, "Orchestrator instructions must be text", OK],
    ),
    (
        Place::Node,
        "instructions",
        r#""""#,
        [OK, "Invalid instructions or network: a", OK],
    ),
    (
        Place::Node,
        "role",
        r#""""#,
        [OK, "Invalid agent role: a", "Invalid agent role: a"],
    ),
    (
        Place::Node,
        "freedom",
        r#""""#,
        [
            OK,
            "Workflow nodes support read_only, write_in_repo, publish or unrestricted",
            "Workflow nodes support read_only, write_in_repo, publish or unrestricted",
        ],
    ),
    (
        Place::Node,
        "execution_mode",
        r#""""#,
        [OK, "Invalid execution mode: a", "Invalid execution mode: a"],
    ),
    (
        Place::Node,
        "session_mode",
        r#""""#,
        [OK, "Invalid session mode: a", "Invalid session mode: a"],
    ),
    (
        Place::Node,
        "max_attempts",
        r#""""#,
        [
            OK,
            "max_attempts must be an integer between 1 and 10000",
            "max_attempts must be an integer between 1 and 10000",
        ],
    ),
    (
        Place::Node,
        "timeout_seconds",
        r#""""#,
        [OK, OK, "timeout_seconds must be a nonnegative integer"],
    ),
    (
        Place::Node,
        "network",
        r#""""#,
        [OK, OK, "Invalid instructions or network: a"],
    ),
    (Place::Node, "title", r#""""#, [OK, POSITION, OK]),
    (Place::Node, "position", "{}", [OK, POSITION, POSITION]),
    (Place::Node, "purpose", r#""""#, [OK, PURPOSE, PURPOSE]),
    (
        Place::Node,
        "resources",
        "[]",
        [
            OK,
            "resource names must be a list of names",
            "request 1 to 16 resource names, got 0",
        ],
    ),
    (
        Place::Candidate,
        "fallbacks",
        "[]",
        [OK, "Fallbacks must be an ordered array", OK],
    ),
    (Place::Candidate, "observe_repos", "[]", [OK, OK, OK]),
    (
        Place::Candidate,
        "allowed_commands",
        "[]",
        [OK, "allowed_commands must be a list of strings", OK],
    ),
    (
        Place::Candidate,
        "model",
        r#""""#,
        [OK, OK, "Model must be a nonempty string"],
    ),
    (
        Place::Candidate,
        "reasoning_effort",
        r#""""#,
        [OK, OK, "Invalid reasoning_effort"],
    ),
    (
        Place::Candidate,
        "background_idle_seconds",
        r#""""#,
        [OK, OK, IDLE],
    ),
    (
        Place::Claude,
        "max_turns",
        r#""""#,
        [OK, OK, "max_turns must be an integer between 1 and 10000"],
    ),
    (
        Place::Connection,
        "condition",
        r#""""#,
        [OK, "Invalid condition or connection flags: c1", OK],
    ),
    (
        Place::OnDemand,
        "title",
        r#""""#,
        [
            OK,
            "On-demand title and call_condition must be text: od",
            OK,
        ],
    ),
    (
        Place::OnDemand,
        "call_condition",
        r#""""#,
        [
            OK,
            "On-demand title and call_condition must be text: od",
            OK,
        ],
    ),
    (
        Place::OnDemand,
        "execution_mode",
        r#""""#,
        [
            OK,
            "Invalid on-demand execution_mode: od",
            "Invalid on-demand execution_mode: od",
        ],
    ),
];

#[test]
fn absent_null_empty_defaults_matrix() {
    let mut failures = Vec::new();
    for (place, field, empty, expected) in ROWS {
        let inputs = [None, Some("null"), Some(*empty)];
        for (value, want) in inputs.iter().zip(expected) {
            let got = outcome(&build(*place, field, *value));
            if got != *want {
                failures.push(format!("{field} {value:?}: got {got:?}, want {want:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn absent_agent_and_null_agent_differ() {
    assert_eq!(outcome(&agent("")), OK);
    assert_eq!(
        outcome(&agent(r#""agent":null"#)),
        "Agent must be an object"
    );
    assert!(outcome(&agent(r#""agent":{}"#)).starts_with("unknown backend ''"));
}
