//! Size bounds (decision D7): 1 MiB of text, 1000 nodes counting on-demand nodes, 4000
//! connections and a JSON depth of 64, each at the limit and just over it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use hf_interfaces::workflow::json_text::{MAX_DEPTH, MAX_TEXT_BYTES, ParseError, parse};

const TOO_MANY_NODES: &str = "Workflow supports at most 1000 nodes, on-demand nodes included";
const TOO_MANY_CONNECTIONS: &str = "Workflow supports at most 4000 connections";

/// A chain of `total` connected nodes (Start, agents, End) and `on_demand` on-demand nodes.
fn chain(total: usize, on_demand: usize) -> String {
    let mut nodes = vec![r#"{"id":"n0","type":"start"}"#.to_string()];
    for index in 1..total - 1 {
        nodes.push(format!(r#"{{"id":"n{index}","type":"agent"}}"#));
    }
    nodes.push(format!(r#"{{"id":"n{}","type":"end"}}"#, total - 1));
    let edges: Vec<String> = (0..total - 1)
        .map(|index| {
            format!(
                r#"{{"id":"c{index}","source":"n{index}","target":"n{}"}}"#,
                index + 1
            )
        })
        .collect();
    let od: Vec<String> = (0..on_demand)
        .map(|index| format!(r#"{{"id":"od{index}","instructions":"x"}}"#))
        .collect();
    format!(
        r#"{{"name":"flow","nodes":[{}],"connections":[{}],"on_demand_nodes":[{}]}}"#,
        nodes.join(","),
        edges.join(","),
        od.join(",")
    )
}

/// Start, one agent and End joined by `edges` parallel connections in explicit routing.
fn wide(edges: usize) -> String {
    let mut links: Vec<String> = (0..edges - 1)
        .map(|index| format!(r#"{{"id":"c{index}","source":"s","target":"a"}}"#))
        .collect();
    links.push(r#"{"id":"last","source":"a","target":"e"}"#.to_string());
    format!(
        r#"{{"name":"flow","routing_mode":"explicit","nodes":[{{"id":"s","type":"start"}},{{"id":"a","type":"agent"}},{{"id":"e","type":"end"}}],"connections":[{}]}}"#,
        links.join(",")
    )
}

#[test]
fn nodes_at_the_limit_pass() {
    let definition = accepted(&chain(1000, 0));
    assert_eq!(
        definition
            .get("nodes")
            .and_then(|nodes| nodes.as_array())
            .map(Vec::len),
        Some(1000)
    );
}

#[test]
fn nodes_just_over_the_limit_fail() {
    assert_eq!(refused(&chain(1001, 0)), TOO_MANY_NODES);
    assert_eq!(refused(&chain(1200, 0)), TOO_MANY_NODES);
}

#[test]
fn on_demand_nodes_count_toward_the_node_limit() {
    assert!(run(&json(&chain(999, 1))).is_ok());
    assert!(run(&json(&chain(3, 997))).is_ok());
    assert_eq!(refused(&chain(1000, 1)), TOO_MANY_NODES);
    assert_eq!(refused(&chain(3, 998)), TOO_MANY_NODES);
}

#[test]
fn node_bound_is_checked_before_node_contents() {
    let garbage = vec!["5"; 1001].join(",");
    let input = format!(r#"{{"name":"flow","nodes":[{garbage}],"connections":[]}}"#);
    assert_eq!(refused(&input), TOO_MANY_NODES);
}

#[test]
fn connections_at_the_limit_pass() {
    let definition = accepted(&wide(4000));
    assert_eq!(
        definition
            .get("connections")
            .and_then(|list| list.as_array())
            .map(Vec::len),
        Some(4000)
    );
}

#[test]
fn connections_just_over_the_limit_fail() {
    assert_eq!(refused(&wide(4001)), TOO_MANY_CONNECTIONS);
}

#[test]
fn connection_bound_is_checked_before_connection_contents() {
    let garbage = vec!["5"; 4001].join(",");
    let input = format!(
        r#"{{"name":"flow","nodes":[{{"id":"s","type":"start"}},{{"id":"e","type":"end"}}],"connections":[{garbage}]}}"#
    );
    assert_eq!(refused(&input), TOO_MANY_CONNECTIONS);
}

#[test]
fn text_size_limit_applies_to_definitions() {
    let head = r#"{"name":"flow","note":""#;
    let tail = r#""}"#;
    let filler = "x".repeat(MAX_TEXT_BYTES - head.len() - tail.len());
    let at_limit = format!("{head}{filler}{tail}");
    assert_eq!(at_limit.len(), MAX_TEXT_BYTES);
    assert!(parse(&at_limit).is_ok());
    let over = format!("{head}{filler}x{tail}");
    assert_eq!(parse(&over), Err(ParseError::TooLarge));
}

#[test]
fn depth_limit_applies_to_definitions() {
    let nested = |depth: usize| {
        format!(
            r#"{{"name":"flow","note":{}0{}}}"#,
            "[".repeat(depth - 1),
            "]".repeat(depth - 1)
        )
    };
    assert!(parse(&nested(MAX_DEPTH)).is_ok());
    assert_eq!(parse(&nested(MAX_DEPTH + 1)), Err(ParseError::TooDeep));
}
