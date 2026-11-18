//! The resource-scope walk memoizes completed (workflow, scope, label) contexts. This is a
//! timing test because `validate_tree_scopes` takes no port to count work through. The
//! shape makes an unmemoized walk orders of magnitude slower than the limit, even in release.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_application::workflow::snapshot_scopes::validate_tree_scopes;
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::tree::{DependencyTree, TreeEntry};
use std::time::{Duration, Instant};

const LEVELS: usize = 4;
const WIDTH: usize = 100;
const LEAF_NODES: usize = 1000;
const LIMIT: Duration = Duration::from_millis(500);

fn object(entries: Vec<(&str, Json)>) -> JsonObject {
    JsonObject::from_entries(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn id_of(level: usize, index: usize) -> String {
    format!("l{level}-{index}")
}

fn call(index: usize, target: &str) -> Json {
    Json::Object(object(vec![
        ("id", Json::text(&format!("call{index}"))),
        ("type", Json::text("workflow")),
        (
            "workflow_ref",
            Json::Object(object(vec![("workflow_id", Json::text(target))])),
        ),
    ]))
}

fn agent(index: usize) -> Json {
    Json::Object(object(vec![
        ("id", Json::text(&format!("agent{index}"))),
        ("type", Json::text("agent")),
    ]))
}

fn entry(level: usize, index: usize) -> (String, TreeEntry) {
    let nodes: Vec<Json> = if level + 1 < LEVELS {
        (0..WIDTH)
            .map(|target| call(target, &id_of(level + 1, target)))
            .collect()
    } else {
        (0..LEAF_NODES).map(agent).collect()
    };
    let id = id_of(level, index);
    let definition = object(vec![
        ("name", Json::text(&id)),
        ("nodes", Json::Array(nodes)),
    ]);
    let entry = TreeEntry {
        name: id.clone(),
        revision: Json::int(1),
        definition_sha256: String::new(),
        definition,
    };
    (id, entry)
}

/// Every workflow at level k references every workflow at level k+1, so an unmemoized walk
/// visits each leaf once per path: WIDTH^(LEVELS-1) times.
fn layered_tree() -> DependencyTree {
    let mut workflows = vec![entry(0, 0)];
    for level in 1..LEVELS {
        workflows.extend((0..WIDTH).map(|index| entry(level, index)));
    }
    DependencyTree {
        root_workflow_id: id_of(0, 0),
        workflows,
        edges: Vec::new(),
        access: Vec::new(),
    }
}

#[test]
fn wide_layered_diamonds_validate_in_time_independent_of_path_count() {
    let tree = layered_tree();
    let started = Instant::now();
    validate_tree_scopes(&tree).unwrap();
    let elapsed = started.elapsed();
    assert!(elapsed < LIMIT, "scope walk took {elapsed:?}");
}
