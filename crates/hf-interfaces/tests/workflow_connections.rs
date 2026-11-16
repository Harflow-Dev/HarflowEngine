//! Connections and the start/end structure of a definition.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use hf_domain::workflow::json::Json;

const NODES: &str =
    r#""nodes":[{"id":"s","type":"start"},{"id":"a","type":"agent"},{"id":"e","type":"end"}]"#;

fn with_connections(connections: &str) -> String {
    format!(r#"{{"name":"flow",{NODES},"connections":[{connections}]}}"#)
}

fn chain(edge: &str) -> String {
    with_connections(&format!(
        r#"{{"id":"c1","source":"s","target":"a"{edge}}},{{"id":"c2","source":"a","target":"e"}}"#
    ))
}

#[test]
fn defaults_are_written() {
    let definition = accepted(&chain(""));
    let edge = at(&definition, "connections.0").unwrap();
    assert_eq!(
        keys(edge),
        ["id", "source", "target", "condition", "default", "backward"]
    );
    assert_eq!(
        at(&definition, "connections.0.condition"),
        Some(&Json::text(""))
    );
    assert_eq!(
        at(&definition, "connections.0.default"),
        Some(&Json::Bool(false))
    );
}

#[test]
fn shape_rules() {
    assert_eq!(
        refused(&with_connections("5")),
        "Every connection must be an object"
    );
    let identifier =
        "Identifiers must contain 1\u{2013}100 letters, digits, dots, dashes or underscores";
    assert_eq!(
        refused(&with_connections(r#"{"source":"s","target":"a"}"#)),
        identifier
    );
    assert_eq!(
        refused(&with_connections(r#"{"id":"","source":"s","target":"a"}"#)),
        identifier
    );
    let twice = r#"{"id":"c","source":"s","target":"a"},{"id":"c","source":"a","target":"e"}"#;
    assert_eq!(refused(&with_connections(twice)), "Duplicate connection c");
}

#[test]
fn endpoints_must_name_nodes() {
    for edge in [
        r#"{"id":"c1","source":"zz","target":"a"}"#,
        r#"{"id":"c1","source":"s","target":"zz"}"#,
        r#"{"id":"c1","source":5,"target":"a"}"#,
        r#"{"id":"c1","target":"a"}"#,
        r#"{"id":"c1","source":null,"target":"a"}"#,
    ] {
        assert_eq!(
            refused(&with_connections(edge)),
            "Connection c1 references an unknown node",
            "{edge}"
        );
    }
}

#[test]
fn end_has_no_outputs_and_start_no_inputs() {
    let message = "End cannot have outputs and Start cannot have inputs";
    let from_end = r#"{"id":"c1","source":"s","target":"a"},{"id":"c2","source":"a","target":"e"},{"id":"c3","source":"e","target":"a"}"#;
    assert_eq!(refused(&with_connections(from_end)), message);
    let to_start = r#"{"id":"c1","source":"s","target":"a"},{"id":"c2","source":"a","target":"e"},{"id":"c3","source":"a","target":"s"}"#;
    assert_eq!(refused(&with_connections(to_start)), message);
}

#[test]
fn retry_cap_and_flags() {
    let retries = "max_retries must be a nonnegative integer: c1";
    for bad in ["-1", "1.5", "true", "null", r#""2""#] {
        assert_eq!(
            refused(&chain(&format!(r#","max_retries":{bad}"#))),
            retries,
            "{bad}"
        );
    }
    for ok in ["0", "3"] {
        assert!(run(&json(&chain(&format!(r#","max_retries":{ok}"#)))).is_ok());
    }
    let flags = "Invalid condition or connection flags: c1";
    for bad in [
        r#","condition":5"#,
        r#","condition":null"#,
        r#","default":"yes""#,
        r#","default":1"#,
        r#","backward":null"#,
        r#","backward":0"#,
    ] {
        assert_eq!(refused(&chain(bad)), flags, "{bad}");
    }
    let kept = accepted(&chain(r#","condition":"ok","default":true"#));
    assert_eq!(
        at(&kept, "connections.0.condition"),
        Some(&Json::text("ok"))
    );
}

#[test]
fn outgoing_and_default_rules() {
    let dangling = r#"{"id":"c1","source":"s","target":"a"}"#;
    assert_eq!(
        refused(&with_connections(dangling)),
        "Node a has no outgoing connection"
    );
    let two_defaults = r#"{"id":"c1","source":"s","target":"a"},{"id":"c2","source":"a","target":"e","default":true},{"id":"c3","source":"a","target":"e","default":true}"#;
    assert_eq!(
        refused(&with_connections(two_defaults)),
        "Node a has multiple default connections"
    );
    let one_default = r#"{"id":"c1","source":"s","target":"a"},{"id":"c2","source":"a","target":"e","default":true},{"id":"c3","source":"a","target":"e"}"#;
    assert!(run(&json(&with_connections(one_default))).is_ok());
}

#[test]
fn start_and_end_counts() {
    let message = "Workflow needs exactly one Start and at least one End";
    let two_starts = r#"{"name":"flow","nodes":[{"id":"s","type":"start"},{"id":"t","type":"start"},{"id":"e","type":"end"}],"connections":[]}"#;
    assert_eq!(refused(two_starts), message);
    let no_end = r#"{"name":"flow","nodes":[{"id":"s","type":"start"},{"id":"a","type":"agent"}],"connections":[]}"#;
    assert_eq!(refused(no_end), message);
    let no_start = r#"{"name":"flow","nodes":[{"id":"a","type":"agent"},{"id":"e","type":"end"}],"connections":[]}"#;
    assert_eq!(refused(no_start), message);
    // The count is checked before any connection is read.
    let garbage = r#"{"name":"flow","nodes":[{"id":"s","type":"start"}],"connections":[5]}"#;
    assert_eq!(refused(garbage), message);
}

#[test]
fn backward_flag_is_inferred() {
    let retry = r#"{"name":"flow","nodes":[{"id":"s","type":"start"},{"id":"a","type":"agent"},{"id":"b","type":"agent"},{"id":"e","type":"end"}],"connections":[{"id":"c1","source":"s","target":"a","backward":true},{"id":"c2","source":"a","target":"b"},{"id":"c3","source":"b","target":"e","default":true},{"id":"c4","source":"b","target":"a","backward":false}]}"#;
    let definition = accepted(retry);
    let flags: Vec<&Json> = (0..4)
        .map(|index| at(&definition, &format!("connections.{index}.backward")).unwrap())
        .collect();
    assert_eq!(
        flags,
        [
            &Json::Bool(false),
            &Json::Bool(false),
            &Json::Bool(false),
            &Json::Bool(true)
        ]
    );
}

#[test]
fn missing_connections_key_leaves_start_dangling() {
    let input = r#"{"name":"flow","nodes":[{"id":"s","type":"start"},{"id":"e","type":"end"}]}"#;
    assert_eq!(refused(input), "Node s has no outgoing connection");
}
