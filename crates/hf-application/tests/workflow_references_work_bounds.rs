//! Resolver memoization bounds the work on a diamond-heavy layered catalog.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_support;

use hf_application::workflow::identity::workflow_identity;
use hf_application::workflow::ports::DefinitionCodec;
use hf_application::workflow::references::resolve_dependencies;
use hf_application::workflow::validate::Validator;
use hf_domain::workflow::json::{Json, JsonObject};
use std::cell::Cell;
use workflow_support::{FakeCodec, MapResolver, TableCatalog};

const LEVELS: usize = 4;
const WIDTH: usize = 10;

fn object(entries: &[(&str, Json)]) -> JsonObject {
    JsonObject::from_entries(
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect(),
    )
}

fn plain(id: &str, kind: &str) -> Json {
    Json::Object(object(&[
        ("id", Json::text(id)),
        ("type", Json::text(kind)),
    ]))
}

fn link(id: &str, source: &str, target: &str) -> Json {
    Json::Object(object(&[
        ("id", Json::text(id)),
        ("source", Json::text(source)),
        ("target", Json::text(target)),
    ]))
}

/// No `workflow_id`: the identity is then derived from the name through `sha256_hex`, which
/// is the one codec call every resolver visit makes and so measures visits.
fn layered(name: &str, targets: &[String]) -> JsonObject {
    let mut nodes = vec![plain("start", "start")];
    for (index, target) in targets.iter().enumerate() {
        nodes.push(Json::Object(object(&[
            ("id", Json::text(&format!("call{index}"))),
            ("type", Json::text("workflow")),
            (
                "workflow_ref",
                Json::Object(object(&[("workflow_id", Json::text(target))])),
            ),
        ])));
    }
    nodes.push(plain("end", "end"));
    let connections = nodes
        .windows(2)
        .enumerate()
        .map(|(index, pair)| {
            let id_of = |node: &Json| node.as_object().unwrap().get("id").unwrap().clone();
            link(
                &format!("e{index}"),
                id_of(&pair[0]).as_str().unwrap(),
                id_of(&pair[1]).as_str().unwrap(),
            )
        })
        .collect();
    object(&[
        ("name", Json::text(name)),
        ("routing_mode", Json::text("explicit")),
        ("nodes", Json::Array(nodes)),
        ("connections", Json::Array(connections)),
    ])
}

fn name_of(level: usize, index: usize) -> String {
    format!("L{level}-{index}")
}

/// Level 0 is the root; every workflow at level k references every workflow at level k+1.
fn catalog() -> Vec<JsonObject> {
    let mut definitions = vec![layered(&name_of(0, 0), &level_ids(1))];
    for level in 1..LEVELS {
        for index in 0..WIDTH {
            let targets = if level + 1 < LEVELS {
                level_ids(level + 1)
            } else {
                Vec::new()
            };
            definitions.push(layered(&name_of(level, index), &targets));
        }
    }
    definitions
}

fn level_ids(level: usize) -> Vec<String> {
    (0..WIDTH)
        .map(|index| workflow_identity(&FakeCodec, &layered(&name_of(level, index), &[])))
        .collect()
}

struct CountingCodec(Cell<usize>);
impl DefinitionCodec for CountingCodec {
    fn sha256_hex(&self, bytes: &[u8]) -> String {
        self.0.set(self.0.get() + 1);
        FakeCodec.sha256_hex(bytes)
    }
    fn definition_sha256(&self, definition: &JsonObject) -> String {
        FakeCodec.definition_sha256(definition)
    }
    fn ascii_dumps_len(&self, value: &Json) -> usize {
        FakeCodec.ascii_dumps_len(value)
    }
}

#[test]
fn diamond_layers_resolve_with_work_bounded_by_distinct_definition_depth_pairs() {
    let catalog_table = TableCatalog;
    let resolver = MapResolver::identity(&[]);
    let validator = Validator::new(&catalog_table, &resolver);
    let stored: Vec<JsonObject> = catalog()
        .iter()
        .map(|definition| {
            validator
                .validate(&Json::Object(definition.clone()))
                .expect("layered fixture is valid")
        })
        .collect();
    let root = stored[0].clone();
    let codec = CountingCodec(Cell::new(0));
    let tree =
        resolve_dependencies(&validator, &codec, &Json::Object(root), &stored, None).unwrap();

    let definitions = 1 + (LEVELS - 1) * WIDTH;
    assert_eq!(tree.workflows.len(), definitions);
    // Each definition sits at exactly one depth, so each is expanded once; references are
    // only followed from the levels above the last.
    let reference_visits = WIDTH + (LEVELS - 2) * WIDTH * WIDTH;
    // One identity hash for each of: a reference entered (reference_visits), a target
    // expanded (definitions - 1), the root's own entry, root frame and root id (3), and
    // indexing the catalog (definitions).
    let expected = reference_visits + (definitions - 1) + 3 + definitions;
    let unmemoized_visits = (1..LEVELS)
        .map(|power| WIDTH.pow(power as u32))
        .sum::<usize>();
    let hashes = codec.0.get();
    assert!(
        hashes >= reference_visits,
        "{hashes} identity hashes is below the {reference_visits} references that must be entered"
    );
    assert_eq!(hashes, expected, "identity hashes");
    assert!(
        unmemoized_visits > 2 * expected,
        "the fixture must be wide enough that an unmemoized walk exceeds the bound"
    );
}
