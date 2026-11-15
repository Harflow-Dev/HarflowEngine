//! Run and node resource declarations.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use hf_domain::workflow::json::Json;

const NOT_LIST: &str = "resource names must be a list of names";

fn names(list: &[&str]) -> Json {
    Json::texts(&list.iter().map(|name| name.to_string()).collect::<Vec<_>>())
}

#[test]
fn node_resources_without_run_declaration_sorted_no_subset() {
    let definition = accepted(&agent(r#""resources":["zz","aa"]"#));
    assert_eq!(
        at(&definition, "nodes.1.resources"),
        Some(&names(&["aa", "zz"]))
    );
    assert!(!definition.contains_key("resources"));
}

#[test]
fn run_declaration_is_sorted_and_bounds_nodes() {
    let definition = accepted(&flow(
        r#""resources":["c3","a1","b2"]"#,
        r#""resources":["b2","a1"]"#,
    ));
    assert_eq!(
        definition.get("resources"),
        Some(&names(&["a1", "b2", "c3"]))
    );
    assert_eq!(
        at(&definition, "nodes.1.resources"),
        Some(&names(&["a1", "b2"]))
    );
    let outside = flow(r#""resources":["a1"]"#, r#""resources":["a1","zz"]"#);
    assert_eq!(
        refused(&outside),
        "Node resources must be a subset of run resources: a"
    );
}

#[test]
fn on_demand_resources_follow_the_run_declaration() {
    let od = r#""on_demand_nodes":[{"id":"od","instructions":"x","resources":["zz"]}]"#;
    assert_eq!(
        refused(&top(&format!(r#""resources":["a1"],{od}"#))),
        "Node resources must be a subset of run resources: od"
    );
    let definition = accepted(&top(&format!(r#""resources":["zz"],{od}"#)));
    assert_eq!(
        at(&definition, "on_demand_nodes.0.resources"),
        Some(&names(&["zz"]))
    );
}

#[test]
fn non_list_resources_refused() {
    for bad in [r#"{"a":1}"#, r#"{}"#, "5", "1.5", "true", "null"] {
        assert_eq!(
            refused(&agent(&format!(r#""resources":{bad}"#))),
            NOT_LIST,
            "node {bad}"
        );
        assert_eq!(
            refused(&top(&format!(r#""resources":{bad}"#))),
            NOT_LIST,
            "run {bad}"
        );
        let od =
            format!(r#""on_demand_nodes":[{{"id":"od","instructions":"x","resources":{bad}}}]"#);
        assert_eq!(refused(&top(&od)), NOT_LIST, "on-demand {bad}");
    }
    assert_eq!(
        refused(&agent(r#""resources":"db""#)),
        "resource names must be a list of names, not a single string"
    );
}

#[test]
fn name_count_bounds() {
    assert_eq!(
        refused(&agent(r#""resources":[]"#)),
        "request 1 to 16 resource names, got 0"
    );
    let sixteen: Vec<String> = (0..16).map(|index| format!("\"r{index}\"")).collect();
    let definition = accepted(&agent(&format!(r#""resources":[{}]"#, sixteen.join(","))));
    assert_eq!(
        at(&definition, "nodes.1.resources")
            .and_then(Json::as_array)
            .map(Vec::len),
        Some(16)
    );
    let seventeen: Vec<String> = (0..17).map(|index| format!("\"r{index}\"")).collect();
    assert_eq!(
        refused(&agent(&format!(r#""resources":[{}]"#, seventeen.join(",")))),
        "request 1 to 16 resource names, got 17"
    );
}

#[test]
fn name_grammar() {
    let long = "a".repeat(65);
    for bad in ["Bad", "-x", "a b", "", &long, "é"] {
        let message = refused(&agent(&format!(r#""resources":["{bad}"]"#)));
        assert!(
            message.starts_with("invalid resource name '"),
            "{bad}: {message}"
        );
        assert!(message.ends_with("(1 to 64 characters, starting with a letter or digit)"));
    }
    assert!(refused(&agent(r#""resources":[5]"#)).starts_with("invalid resource name 5:"));
    assert!(refused(&agent(r#""resources":[null]"#)).starts_with("invalid resource name None:"));
    let allowed = "a".repeat(64);
    for good in ["a", "db:main.1", "x_y-z", &allowed] {
        assert!(
            run(&json(&agent(&format!(r#""resources":["{good}"]"#)))).is_ok(),
            "{good}"
        );
    }
    assert_eq!(
        refused(&agent(r#""resources":["a","b","a"]"#)),
        "duplicate resource names in one request"
    );
}

#[test]
fn only_agent_nodes_declare_resources() {
    let run_node =
        r#"{"id":"a","type":"workflow","workflow_ref":{"workflow_id":"w"},"resources":["x"]}"#;
    assert_eq!(
        refused(&explicit_flow(run_node)),
        "Only agent nodes accept resources: a"
    );
    let start = r#"{"name":"flow","nodes":[{"id":"s","type":"start","resources":["x"]},{"id":"e","type":"end"}],"connections":[{"id":"c","source":"s","target":"e"}]}"#;
    assert_eq!(refused(start), "Only agent nodes accept resources: s");
}

#[test]
fn prefer_subagent_cannot_declare_resources() {
    assert_eq!(
        refused(&agent(
            r#""resources":["x"],"execution_mode":"prefer_subagent""#
        )),
        "prefer_subagent nodes cannot declare resources: a"
    );
    let invalid_first = agent(r#""resources":["Bad"],"execution_mode":"prefer_subagent""#);
    assert!(refused(&invalid_first).starts_with("invalid resource name"));
    let od = r#""on_demand_nodes":[{"id":"od","instructions":"x","resources":["x"],"execution_mode":"prefer_subagent"}]"#;
    assert_eq!(
        refused(&top(od)),
        "prefer_subagent nodes cannot declare resources: od"
    );
}
