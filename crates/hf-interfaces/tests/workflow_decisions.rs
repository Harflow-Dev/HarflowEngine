//! Decided fixes of Python behaviour: crash inputs are ordinary refusals (D1), lone
//! surrogates are refused (D3), outcomes do not depend on hash order (D4) and non-list
//! resources are refused (D5).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use hf_interfaces::workflow::json_text::{ParseError, parse};

const UNHASHABLE: [&str; 3] = ["[]", "{}", r#"[{"a":[1]}]"#];

fn assert_refused_for_each(build: impl Fn(&str) -> String, expected: &str) {
    for value in UNHASHABLE {
        assert_eq!(refused(&build(value)), expected, "{value}");
    }
}

#[test]
fn d1_unhashable_node_fields_refused() {
    for (field, message) in [
        ("role", "Invalid agent role: a"),
        (
            "freedom",
            "Workflow nodes support read_only, write_in_repo, publish or unrestricted",
        ),
        ("execution_mode", "Invalid execution mode: a"),
        ("session_mode", "Invalid session mode: a"),
        ("branch_mode", "Invalid branching mode: a"),
    ] {
        assert_refused_for_each(|value| agent(&format!(r#""{field}":{value}"#)), message);
    }
    assert_refused_for_each(
        |value| node_flow("", &format!(r#"{{"id":"a","type":{value}}}"#)),
        "Invalid node type: a",
    );
}

#[test]
fn d1_unhashable_candidate_and_top_level_fields_refused() {
    assert_refused_for_each(
        |value| with_agent(&format!(r#""reasoning_effort":{value}"#)),
        "Invalid reasoning_effort",
    );
    assert_refused_for_each(
        |value| with_agent(&format!(r#""conversation_profile":{value}"#)),
        "conversation_profile must be native or shipped-medium35-high",
    );
    assert_refused_for_each(
        |value| top(&format!(r#""context_delivery":{value}"#)),
        "context_delivery must be legacy or optimized_v1",
    );
    assert_refused_for_each(
        |value| top(&format!(r#""routing_mode":{value}"#)),
        "Invalid routing_mode",
    );
    assert_refused_for_each(
        |value| with_agent(&format!(r#""model":{value}"#)),
        "Model must be a nonempty string",
    );
    for value in UNHASHABLE {
        let message = refused(&with_agent(&format!(r#""backend":{value}"#)));
        assert!(message.starts_with("unknown backend "), "{message}");
    }
}

#[test]
fn d1_unhashable_graph_and_reference_fields_refused() {
    let edge = |value: &str| {
        let input = r#"{"name":"flow","nodes":[{"id":"s","type":"start"},{"id":"e","type":"end"}],"connections":[{"id":"c1","source":"s","target":"e"}]}"#;
        input.replace(r#""source":"s""#, &format!(r#""source":{value}"#))
    };
    assert_refused_for_each(edge, "Connection c1 references an unknown node");
    let run_node = |field: &str, value: &str| {
        explicit_flow(&format!(
            r#"{{"id":"a","type":"workflow","workflow_ref":{{"workflow_id":"w"}},"{field}":{value}}}"#
        ))
    };
    assert_refused_for_each(
        |value| run_node("orchestrator_mode", value),
        "Run workflow orchestrator_mode must be child or current: a",
    );
    assert_refused_for_each(
        |value| run_node("child_session_policy", value),
        "Invalid child_session_policy: a",
    );
    assert_refused_for_each(
        |value| {
            explicit_flow(&format!(
                r#"{{"id":"a","type":"workflow","workflow_ref":{{"workflow_id":{value}}}}}"#
            ))
        },
        "Run workflow node requires workflow_ref.workflow_id: a",
    );
    assert_refused_for_each(
        |value| {
            top(&format!(
                r#""on_demand_nodes":[{{"id":"od","instructions":"x","freedom":{value}}}]"#
            ))
        },
        "Invalid on-demand freedom or network: od",
    );
    assert_refused_for_each(
        |value| {
            top(&format!(
                r#""on_demand_nodes":[{{"id":"od","instructions":"x","session_mode":{value}}}]"#
            ))
        },
        "Invalid on-demand session_mode: od",
    );
}

#[test]
fn d1_unhashable_parallel_fields_refused() {
    let group = |field: &str, value: &str| {
        format!(
            r#"{{"name":"flow","routing_mode":"explicit","nodes":[{{"id":"s","type":"start"}},{{"id":"p","type":"parallel_start","parallel_group_id":"g","{field}":{value}}},{{"id":"x","type":"agent"}},{{"id":"y","type":"agent"}},{{"id":"q","type":"parallel_end","parallel_group_id":"g"}},{{"id":"e","type":"end"}}],"connections":[{{"id":"c1","source":"s","target":"p"}},{{"id":"c2","source":"p","target":"x"}},{{"id":"c3","source":"p","target":"y"}},{{"id":"c4","source":"x","target":"q"}},{{"id":"c5","source":"y","target":"q"}},{{"id":"c6","source":"q","target":"e"}}]}}"#
        )
    };
    assert_refused_for_each(
        |value| group("branch_selection", value),
        "Parallel branch_selection must be all or orchestrator",
    );
    let identifier =
        "Identifiers must contain 1\u{2013}100 letters, digits, dots, dashes or underscores";
    for value in UNHASHABLE {
        let input = group("x", "1").replace(
            r#""parallel_group_id":"g","x":1"#,
            &format!(r#""parallel_group_id":{value}"#),
        );
        assert_eq!(refused(&input), identifier, "{value}");
    }
}

#[test]
fn d1_overflowing_numbers_are_refused() {
    let huge = "9".repeat(400);
    let position = "Invalid title or canvas position (x and y must be finite and nonnegative): a";
    assert_eq!(
        refused(&agent(&format!(r#""position":{{"x":{huge},"y":1}}"#))),
        position
    );
    assert_eq!(
        refused(&agent(&format!(r#""position":{{"x":1,"y":{huge}}}"#))),
        position
    );
    let idle = "background_idle_seconds must be a finite positive number no greater than 21600";
    assert_eq!(
        refused(&with_agent(&format!(r#""background_idle_seconds":{huge}"#))),
        idle
    );
    assert_eq!(
        refused(&top(&format!(r#""max_parallel":{huge}"#))),
        "max_parallel must be an integer between 1 and 64"
    );
    assert_eq!(
        refused(&agent(&format!(r#""max_attempts":{huge}"#))),
        "max_attempts must be an integer between 1 and 10000"
    );
}

#[test]
fn d1_unknown_backend_is_an_ordinary_refusal() {
    let known = "['antigravity', 'claude', 'codex', 'cursor', 'opencode', 'vibe']";
    let expected = format!("unknown backend 'nope'; expected one of {known}");
    assert_eq!(refused(&with_agent(r#""backend":"nope""#)), expected);
    assert_eq!(
        refused(&top(r#""orchestrator":{"backend":"nope"}"#)),
        expected
    );
    assert_eq!(
        refused(&with_agent(
            r#""backend":"codex","fallbacks":[{"backend":"nope"}]"#
        )),
        expected
    );
    assert_eq!(
        refused(&top(
            r#""on_demand_nodes":[{"id":"od","instructions":"x","agent":{"backend":"nope"}}]"#
        )),
        expected
    );
}

#[test]
fn d3_lone_surrogates_never_reach_validation() {
    for text in [
        r#"{"name":"\ud800","nodes":[],"connections":[]}"#,
        r#"{"name":"flow","k\udc00":1}"#,
        r#"{"name":"flow","note":"\ud83d"}"#,
        r#"{"name":"flow","note":"\ude00\ud83d"}"#,
    ] {
        assert_eq!(parse(text), Err(ParseError::LoneSurrogate), "{text}");
    }
    let pair = parse(r#"{"name":"flow","note":"😀"}"#).unwrap();
    assert!(pair.as_object().is_some());
}

#[test]
fn d4_first_invalid_node_wins() {
    let two = |first: &str, second: &str| {
        format!(
            r#"{{"name":"flow","nodes":[{{"id":"s","type":"start"}},{first},{second},{{"id":"e","type":"end"}}],"connections":[{{"id":"c1","source":"s","target":"e"}}]}}"#
        )
    };
    let bad_role = r#"{"id":"a","type":"agent","role":"boss"}"#;
    let bad_mode = r#"{"id":"b","type":"agent","execution_mode":"batch"}"#;
    for _ in 0..20 {
        assert_eq!(refused(&two(bad_role, bad_mode)), "Invalid agent role: a");
        assert_eq!(
            refused(&two(bad_mode, bad_role)),
            "Invalid execution mode: b"
        );
    }
}

#[test]
fn d4_first_declared_group_wins() {
    let groups = |first: &str, second: &str| {
        format!(
            r#"{{"name":"flow","routing_mode":"explicit","nodes":[{{"id":"s","type":"start"}},{{"id":"{first}","type":"parallel_start","parallel_group_id":"g1"}},{{"id":"{second}","type":"parallel_start","parallel_group_id":"g2"}},{{"id":"e","type":"end"}}],"connections":[{{"id":"c1","source":"s","target":"{first}"}},{{"id":"c2","source":"{first}","target":"{second}"}},{{"id":"c3","source":"{second}","target":"e"}}]}}"#
        )
    };
    let prefix = "Parallel group g1 requires matching start and end";
    for _ in 0..20 {
        assert_eq!(refused(&groups("p1", "p2")), prefix);
    }
    let swapped = groups("p1", "p2")
        .replace(r#""parallel_group_id":"g1""#, "X")
        .replace(r#""parallel_group_id":"g2""#, r#""parallel_group_id":"g1""#)
        .replace("X", r#""parallel_group_id":"g2""#);
    assert_eq!(
        refused(&swapped),
        "Parallel group g2 requires matching start and end"
    );
}

#[test]
fn d4_first_unconverged_split_wins() {
    let nodes = |a: &str, b: &str| {
        format!(
            r#"{{"name":"flow","nodes":[{{"id":"s","type":"start"}},{{"id":"{a}","type":"agent"}},{{"id":"{b}","type":"agent"}},{{"id":"x1","type":"agent"}},{{"id":"x2","type":"agent"}},{{"id":"y1","type":"agent"}},{{"id":"y2","type":"agent"}},{{"id":"e1","type":"end"}},{{"id":"e2","type":"end"}},{{"id":"e3","type":"end"}},{{"id":"e4","type":"end"}}],"connections":[{{"id":"c0","source":"s","target":"{a}"}},{{"id":"c1","source":"{a}","target":"x1"}},{{"id":"c2","source":"{a}","target":"x2"}},{{"id":"c3","source":"x1","target":"e1"}},{{"id":"c4","source":"x2","target":"e2"}},{{"id":"c5","source":"x1","target":"{b}"}},{{"id":"c6","source":"{b}","target":"y1"}},{{"id":"c7","source":"{b}","target":"y2"}},{{"id":"c8","source":"y1","target":"e3"}},{{"id":"c9","source":"y2","target":"e4"}}]}}"#
        )
    };
    for _ in 0..20 {
        assert_eq!(
            refused(&nodes("a", "b")),
            "Parallel branches from a must converge at one common node before ending"
        );
        assert_eq!(
            refused(&nodes("b", "a")),
            "Parallel branches from b must converge at one common node before ending"
        );
    }
}

#[test]
fn d4_checks_run_in_a_fixed_order() {
    let all_bad = r#"{"name":"bad/name","context_delivery":5,"routing_mode":5,"description":5,"orchestrator":5,"max_parallel":0,"nodes":[],"connections":[]}"#;
    let steps = [
        (
            "name",
            r#""name":"bad/name""#,
            r#""name":"flow""#,
            "Workflow names must contain",
        ),
        (
            "context_delivery",
            r#""context_delivery":5"#,
            r#""context_delivery":"legacy""#,
            "context_delivery must be",
        ),
        (
            "routing_mode",
            r#""routing_mode":5"#,
            r#""routing_mode":"explicit""#,
            "Invalid routing_mode",
        ),
        (
            "description",
            r#""description":5"#,
            r#""description":"d""#,
            "Description must be text",
        ),
        (
            "orchestrator",
            r#""orchestrator":5"#,
            r#""orchestrator":{"backend":"codex"}"#,
            "Orchestrator must be an agent object",
        ),
        (
            "limits",
            r#""max_parallel":0"#,
            r#""max_parallel":4"#,
            "max_parallel must be an integer",
        ),
    ];
    let mut current = all_bad.to_string();
    for (label, bad, fixed, prefix) in steps {
        let message = refused(&current);
        assert!(message.starts_with(prefix), "{label}: {message}");
        current = current.replace(bad, fixed);
    }
    assert_eq!(
        refused(&current),
        "Workflow requires nodes and connections arrays"
    );
}

#[test]
fn d4_nodes_are_checked_before_connections() {
    let input = r#"{"name":"flow","nodes":[{"id":"s","type":"start"},{"id":"a","type":"robot"},{"id":"e","type":"end"}],"connections":[5]}"#;
    assert_eq!(refused(input), "Invalid node type: a");
}

#[test]
fn d4_listed_ids_and_settings_are_sorted() {
    assert_eq!(
        refused(&with_agent(r#""zz":1,"mm":2,"aa":3"#)),
        "Unknown agent settings: ['aa', 'mm', 'zz']"
    );
    let input = r#"{"name":"flow","nodes":[{"id":"s","type":"start"},{"id":"z","type":"agent"},{"id":"m","type":"agent"},{"id":"e","type":"end"}],"connections":[{"id":"c1","source":"s","target":"z"},{"id":"c2","source":"s","target":"e"},{"id":"c3","source":"z","target":"m"},{"id":"c4","source":"m","target":"z"}]}"#;
    assert_eq!(
        refused(input),
        "Every node must have a forward path to End: m, z"
    );
}

#[test]
fn d5_non_list_resources_refused() {
    for bad in [r#"{"a":1}"#, "5", "true"] {
        assert_eq!(
            refused(&agent(&format!(r#""resources":{bad}"#))),
            "resource names must be a list of names",
            "{bad}"
        );
    }
}
