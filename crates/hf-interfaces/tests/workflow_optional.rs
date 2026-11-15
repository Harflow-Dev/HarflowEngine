//! Optional nodes: they need a safe parallel branch with a required sibling.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use hf_domain::workflow::json::Json;

const UNSAFE: &str = "Optional nodes need a safe parallel branch with a required sibling and no required successor before convergence: ";

fn explicit_group(x: &str, y: &str, tail: &str) -> String {
    format!(
        r#"{{"name":"flow","routing_mode":"explicit","nodes":[{{"id":"s","type":"start"}},{{"id":"p","type":"parallel_start","parallel_group_id":"g"}},{{"id":"x","type":"agent"{x}}},{{"id":"y","type":"agent"{y}}},{{"id":"z","type":"agent"{tail}}},{{"id":"q","type":"parallel_end","parallel_group_id":"g"}},{{"id":"e","type":"end"}}],"connections":[{{"id":"c1","source":"s","target":"p"}},{{"id":"c2","source":"p","target":"x"}},{{"id":"c3","source":"p","target":"y"}},{{"id":"c4","source":"x","target":"z"}},{{"id":"c5","source":"y","target":"q"}},{{"id":"c6","source":"z","target":"q"}},{{"id":"c7","source":"q","target":"e"}}]}}"#
    )
}

const OPTIONAL: &str = r#","optional":true"#;

#[test]
fn optional_branch_with_required_sibling_is_safe() {
    let definition = accepted(&explicit_group(OPTIONAL, "", OPTIONAL));
    assert_eq!(at(&definition, "nodes.2.optional"), Some(&Json::Bool(true)));
    assert_eq!(
        at(&definition, "nodes.3.optional"),
        Some(&Json::Bool(false))
    );
    assert_eq!(at(&definition, "nodes.4.optional"), Some(&Json::Bool(true)));
}

#[test]
fn optional_with_a_required_successor_is_unsafe() {
    // x is optional but its successor z is required before the convergence at q.
    assert_eq!(
        refused(&explicit_group(OPTIONAL, "", "")),
        UNSAFE.to_string() + "x"
    );
}

#[test]
fn optional_branch_without_a_required_sibling_is_unsafe() {
    let both = explicit_group(OPTIONAL, OPTIONAL, r#","optional":true"#);
    assert_eq!(refused(&both), UNSAFE.to_string() + "x, y, z");
}

#[test]
fn unsafe_ids_are_listed_sorted_not_by_node_order() {
    let input = explicit_group(OPTIONAL, OPTIONAL, r#","optional":true"#)
        .replace(
            r#"{"id":"x","type":"agent","optional":true}"#,
            r#"{"id":"b","type":"agent","optional":true}"#,
        )
        .replace(
            r#"{"id":"y","type":"agent","optional":true}"#,
            r#"{"id":"a","type":"agent","optional":true}"#,
        )
        .replace(r#""target":"x""#, r#""target":"b""#)
        .replace(r#""target":"y""#, r#""target":"a""#)
        .replace(r#""source":"x""#, r#""source":"b""#)
        .replace(r#""source":"y""#, r#""source":"a""#);
    assert_eq!(refused(&input), UNSAFE.to_string() + "a, b, z");
}

fn legacy_split(b: &str, c: &str) -> String {
    format!(
        r#"{{"name":"flow","nodes":[{{"id":"s","type":"start"}},{{"id":"a","type":"agent"}},{{"id":"b","type":"agent"{b}}},{{"id":"c","type":"agent"{c}}},{{"id":"j","type":"agent"}},{{"id":"e","type":"end"}}],"connections":[{{"id":"c0","source":"s","target":"a"}},{{"id":"c1","source":"a","target":"b"}},{{"id":"c2","source":"a","target":"c"}},{{"id":"c3","source":"b","target":"j"}},{{"id":"c4","source":"c","target":"j"}},{{"id":"c5","source":"j","target":"e"}}]}}"#
    )
}

#[test]
fn legacy_split_accepts_one_optional_branch() {
    assert!(run(&json(&legacy_split(OPTIONAL, ""))).is_ok());
    assert!(run(&json(&legacy_split("", OPTIONAL))).is_ok());
}

#[test]
fn legacy_split_refuses_two_optional_branches() {
    assert_eq!(
        refused(&legacy_split(OPTIONAL, OPTIONAL)),
        UNSAFE.to_string() + "b, c"
    );
}

#[test]
fn optional_run_workflow_node_counts() {
    let input = r#"{"name":"flow","routing_mode":"explicit","nodes":[{"id":"s","type":"start"},{"id":"p","type":"parallel_start","parallel_group_id":"g"},{"id":"w","type":"workflow","workflow_ref":{"workflow_id":"w1"},"optional":true},{"id":"y","type":"agent"},{"id":"q","type":"parallel_end","parallel_group_id":"g"},{"id":"e","type":"end"}],"connections":[{"id":"c1","source":"s","target":"p"},{"id":"c2","source":"p","target":"w"},{"id":"c3","source":"p","target":"y"},{"id":"c4","source":"w","target":"q"},{"id":"c5","source":"y","target":"q"},{"id":"c6","source":"q","target":"e"}]}"#;
    assert!(run(&json(input)).is_ok());
    assert_eq!(
        refused(&input.replace(
            r#""id":"y","type":"agent""#,
            r#""id":"y","type":"agent","optional":true"#
        )),
        UNSAFE.to_string() + "w, y"
    );
}

#[test]
fn explicit_non_split_optional_node_is_unsafe() {
    let input = r#"{"name":"flow","routing_mode":"explicit","nodes":[{"id":"s","type":"start"},{"id":"a","type":"agent","optional":true},{"id":"b","type":"agent"},{"id":"e","type":"end"}],"connections":[{"id":"c1","source":"s","target":"a"},{"id":"c2","source":"s","target":"b"},{"id":"c3","source":"a","target":"e"},{"id":"c4","source":"b","target":"e"}]}"#;
    assert_eq!(refused(input), UNSAFE.to_string() + "a");
}
