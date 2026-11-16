//! Validation and reading do bounded work on adversarial but legal inputs. The limits are
//! generous for a debug build and far below the earlier quadratic behaviour.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use std::time::{Duration, Instant};

use hf_interfaces::workflow::json_text::{MAX_TEXT_BYTES, parse};

const LIMIT: Duration = Duration::from_secs(5);

fn timed<T>(label: &str, work: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let result = work();
    let elapsed = started.elapsed();
    eprintln!("{label}: {elapsed:?}");
    assert!(elapsed < LIMIT, "{label} took {elapsed:?}");
    result
}

/// Start, `agents` chained agents and End; `duplicates` connections from Start to the first
/// agent; agent `optional` (if any) is optional.
fn fan_in_chain(duplicates: usize, agents: usize, optional: Option<usize>) -> String {
    let mut nodes = vec![r#"{"id":"s","type":"start"}"#.to_string()];
    for index in 0..agents {
        let flag = if optional == Some(index) {
            r#","optional":true"#
        } else {
            ""
        };
        nodes.push(format!(r#"{{"id":"n{index}","type":"agent"{flag}}}"#));
    }
    nodes.push(r#"{"id":"e","type":"end"}"#.to_string());
    let mut edges: Vec<String> = (0..duplicates)
        .map(|index| format!(r#"{{"id":"d{index}","source":"s","target":"n0"}}"#))
        .collect();
    for index in 0..agents {
        let target = if index + 1 == agents {
            "e".to_string()
        } else {
            format!("n{}", index + 1)
        };
        edges.push(format!(
            r#"{{"id":"k{index}","source":"n{index}","target":"{target}"}}"#
        ));
    }
    format!(
        r#"{{"name":"flow","nodes":[{}],"connections":[{}]}}"#,
        nodes.join(","),
        edges.join(",")
    )
}

/// An explicit parallel start with `branches` one-node branches; branch 0 is optional.
fn wide_group(branches: usize) -> String {
    let mut nodes = vec![
        r#"{"id":"s","type":"start"}"#.to_string(),
        r#"{"id":"p","type":"parallel_start","parallel_group_id":"g"}"#.to_string(),
    ];
    let mut edges = vec![r#"{"id":"cs","source":"s","target":"p"}"#.to_string()];
    for index in 0..branches {
        let flag = if index == 0 {
            r#","optional":true"#
        } else {
            ""
        };
        nodes.push(format!(r#"{{"id":"x{index}","type":"agent"{flag}}}"#));
        edges.push(format!(
            r#"{{"id":"a{index}","source":"p","target":"x{index}"}}"#
        ));
        edges.push(format!(
            r#"{{"id":"b{index}","source":"x{index}","target":"q"}}"#
        ));
    }
    nodes.push(r#"{"id":"q","type":"parallel_end","parallel_group_id":"g"}"#.to_string());
    nodes.push(r#"{"id":"e","type":"end"}"#.to_string());
    edges.push(r#"{"id":"cq","source":"q","target":"e"}"#.to_string());
    format!(
        r#"{{"name":"flow","routing_mode":"explicit","nodes":[{}],"connections":[{}]}}"#,
        nodes.join(","),
        edges.join(",")
    )
}

#[test]
fn duplicate_connections_into_a_long_chain_with_an_optional_node() {
    let input = json(&fan_in_chain(3000, 988, Some(500)));
    let outcome = timed("3000 duplicate connections, 990-node chain", || run(&input));
    let message = outcome.unwrap_err();
    assert!(
        message.starts_with("Optional nodes need a safe parallel branch"),
        "{message}"
    );
}

#[test]
fn duplicate_connections_into_a_long_chain_without_optional_nodes() {
    let input = json(&fan_in_chain(3000, 988, None));
    let outcome = timed("3000 duplicate connections, no optional", || run(&input));
    assert!(outcome.is_ok(), "{:?}", outcome.err());
}

#[test]
fn a_wide_parallel_group_with_an_optional_branch() {
    let input = json(&wide_group(900));
    let outcome = timed("900 single-node branches", || run(&input));
    assert!(outcome.is_ok(), "{:?}", outcome.err());
}

#[test]
fn an_object_with_sixty_thousand_keys_is_read_quickly() {
    let entries: Vec<String> = (0..60_000)
        .map(|index| format!(r#""key{index:05}":null"#))
        .collect();
    let text = format!("{{{}}}", entries.join(","));
    assert!(
        text.len() < MAX_TEXT_BYTES && text.len() > 900_000,
        "{}",
        text.len()
    );
    let value = timed("60000 keys", || parse(&text));
    assert_eq!(
        value.unwrap().as_object().map(|map| map.len()),
        Some(60_000)
    );
}

#[test]
fn repeated_keys_replace_in_place_quickly() {
    let entries: Vec<String> = (0..60_000)
        .map(|index| format!(r#""k{}":{index}"#, index % 3))
        .collect();
    let text = format!("{{{}}}", entries.join(","));
    let value = timed("60000 repeated keys", || parse(&text)).unwrap();
    let map = value.as_object().unwrap();
    assert_eq!(map.keys().collect::<Vec<_>>(), ["k0", "k1", "k2"]);
}

/// A chain of 1000 nodes whose list is `[start, end, n998, ..., n1]`.
fn reversed_chain() -> String {
    let mut nodes = vec![
        r#"{"id":"s","type":"start"}"#.to_string(),
        r#"{"id":"e","type":"end"}"#.to_string(),
    ];
    for index in (1..=998).rev() {
        nodes.push(format!(r#"{{"id":"n{index}","type":"agent"}}"#));
    }
    let mut edges = vec![r#"{"id":"k0","source":"s","target":"n1"}"#.to_string()];
    for index in 1..=998 {
        let target = if index == 998 {
            "e".to_string()
        } else {
            format!("n{}", index + 1)
        };
        edges.push(format!(
            r#"{{"id":"k{index}","source":"n{index}","target":"{target}"}}"#
        ));
    }
    format!(
        r#"{{"name":"flow","nodes":[{}],"connections":[{}]}}"#,
        nodes.join(","),
        edges.join(",")
    )
}

#[test]
fn a_reverse_listed_chain_of_1000_nodes_validates_quickly() {
    let input = json(&reversed_chain());
    let outcome = timed("reverse-listed 1000-node chain", || run(&input));
    let definition = outcome.unwrap_or_else(|message| panic!("refused: {message}"));
    assert_eq!(
        definition
            .get("nodes")
            .and_then(|nodes| nodes.as_array())
            .map(Vec::len),
        Some(1000)
    );
}
