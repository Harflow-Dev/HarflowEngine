//! Graph rules through the validator: reachability, loops, legacy splits and joins.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use hf_domain::workflow::json::Json;

/// Agent nodes named by `ids`, Start `s` and End `e` first and last.
fn graph(ids: &[&str], edges: &[(&str, &str)]) -> String {
    let agents: Vec<String> = ids
        .iter()
        .map(|id| format!(r#"{{"id":"{id}","type":"agent"}}"#))
        .collect();
    let nodes = format!(
        r#"{{"id":"s","type":"start"}},{},{{"id":"e","type":"end"}}"#,
        agents.join(",")
    );
    let links: Vec<String> = edges
        .iter()
        .enumerate()
        .map(|(index, (source, target))| {
            format!(r#"{{"id":"c{index}","source":"{source}","target":"{target}"}}"#)
        })
        .collect();
    format!(
        r#"{{"name":"flow","nodes":[{nodes}],"connections":[{}]}}"#,
        links.join(",")
    )
}

#[test]
fn unreachable_node_refused() {
    let input = graph(&["a", "x"], &[("s", "a"), ("a", "e"), ("x", "e")]);
    assert_eq!(refused(&input), "All nodes must be reachable from Start");
}

#[test]
fn retry_loop_marks_backward_edge() {
    let input = graph(
        &["a", "b"],
        &[("s", "a"), ("a", "b"), ("b", "a"), ("b", "e")],
    );
    let definition = accepted(&input);
    let backward =
        |index: usize| at(&definition, &format!("connections.{index}.backward")).cloned();
    assert_eq!(backward(2), Some(Json::Bool(true)));
    assert_eq!(backward(1), Some(Json::Bool(false)));
}

#[test]
fn ambiguous_loop_refused() {
    let input = graph(
        &["a", "b"],
        &[
            ("s", "a"),
            ("s", "b"),
            ("a", "b"),
            ("b", "a"),
            ("a", "e"),
            ("b", "e"),
        ],
    );
    assert_eq!(
        refused(&input),
        "Ambiguous loop: cycles must return to a step that dominates the retrying step"
    );
}

#[test]
fn nodes_without_a_forward_path_to_end_are_listed_sorted() {
    let input = graph(
        &["b", "a"],
        &[("s", "b"), ("s", "e"), ("b", "a"), ("a", "b")],
    );
    assert_eq!(
        refused(&input),
        "Every node must have a forward path to End: a, b"
    );
}

#[test]
fn legacy_split_gets_the_join_of_its_convergence() {
    let input = graph(
        &["a", "b", "c", "j"],
        &[
            ("s", "a"),
            ("a", "b"),
            ("a", "c"),
            ("b", "j"),
            ("c", "j"),
            ("j", "e"),
        ],
    );
    let definition = accepted(&input);
    assert_eq!(at(&definition, "nodes.1.join_id"), Some(&Json::text("j")));
    assert_eq!(at(&definition, "nodes.2.join_id"), None);
}

#[test]
fn legacy_single_forward_join_id_removed() {
    let input = graph(&["a", "b"], &[("s", "a"), ("a", "b"), ("b", "e")]).replace(
        r#"{"id":"b","type":"agent"}"#,
        r#"{"id":"b","type":"agent","join_id":"stale"}"#,
    );
    let definition = accepted(&input);
    assert_eq!(at(&definition, "nodes.2.join_id"), None);
}

#[test]
fn legacy_split_must_converge() {
    let input = graph(
        &["a", "b", "c"],
        &[("s", "a"), ("a", "b"), ("a", "c"), ("b", "e"), ("c", "e")],
    );
    assert!(
        run(&json(&input)).is_ok(),
        "converging at End is a convergence"
    );
    let two_ends = r#"{"name":"flow","nodes":[{"id":"s","type":"start"},{"id":"a","type":"agent"},{"id":"b","type":"agent"},{"id":"c","type":"agent"},{"id":"e","type":"end"},{"id":"f","type":"end"}],"connections":[{"id":"c0","source":"s","target":"a"},{"id":"c1","source":"a","target":"b"},{"id":"c2","source":"a","target":"c"},{"id":"c3","source":"b","target":"e"},{"id":"c4","source":"c","target":"f"}]}"#;
    assert_eq!(
        refused(two_ends),
        "Parallel branches from a must converge at one common node before ending"
    );
}

fn with_join(split_join: &str, join_kind: &str) -> String {
    format!(
        r#"{{"name":"flow","nodes":[{{"id":"s","type":"start"}},{{"id":"a","type":"agent"{split_join}}},{{"id":"b","type":"agent"}},{{"id":"c","type":"agent"}},{{"id":"j","type":"agent"}},{{"id":"jn","type":"{join_kind}"}},{{"id":"e","type":"end"}}],"connections":[{{"id":"c0","source":"s","target":"a"}},{{"id":"c1","source":"a","target":"b"}},{{"id":"c2","source":"a","target":"c"}},{{"id":"c3","source":"b","target":"j"}},{{"id":"c4","source":"c","target":"j"}},{{"id":"c5","source":"j","target":"jn"}},{{"id":"c6","source":"jn","target":"e"}}]}}"#
    )
}

#[test]
fn legacy_join_nodes() {
    assert_eq!(
        refused(&with_join("", "join")),
        "Join jn is outside its matching split"
    );
    let named = refused(&with_join(r#","join_id":"jn""#, "join"));
    assert_eq!(
        named,
        "Cross-branch connection at split a: its legacy Join follows an earlier convergence; connect branches directly to their first shared node"
    );
    // A non-Join node named by a stale join_id is overwritten, not refused.
    let plain = accepted(&with_join(r#","join_id":"jn""#, "agent"));
    assert_eq!(at(&plain, "nodes.1.join_id"), Some(&Json::text("j")));
}

#[test]
fn legacy_convergence_at_a_join_node() {
    let input = r#"{"name":"flow","nodes":[{"id":"s","type":"start"},{"id":"a","type":"agent"},{"id":"b","type":"agent"},{"id":"c","type":"agent"},{"id":"jn","type":"join"},{"id":"e","type":"end"}],"connections":[{"id":"c0","source":"s","target":"a"},{"id":"c1","source":"a","target":"b"},{"id":"c2","source":"a","target":"c"},{"id":"c3","source":"b","target":"jn"},{"id":"c4","source":"c","target":"jn"},{"id":"c5","source":"jn","target":"e"}]}"#;
    let definition = accepted(input);
    assert_eq!(at(&definition, "nodes.1.join_id"), Some(&Json::text("jn")));
}

#[test]
fn unhashable_join_id_is_overwritten() {
    for stale in ["[]", "{}", "5", "null"] {
        let input = graph(
            &["a", "b", "c"],
            &[("s", "a"), ("a", "b"), ("a", "c"), ("b", "e"), ("c", "e")],
        )
        .replace(
            r#"{"id":"a","type":"agent"}"#,
            &format!(r#"{{"id":"a","type":"agent","join_id":{stale}}}"#),
        );
        let definition = accepted(&input);
        assert_eq!(
            at(&definition, "nodes.1.join_id"),
            Some(&Json::text("e")),
            "{stale}"
        );
    }
}
