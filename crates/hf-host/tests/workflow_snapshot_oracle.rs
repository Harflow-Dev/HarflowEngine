//! Independent launch and digest oracle cases for pure A11 snapshots.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_parity_support;
mod workflow_snapshot_support;

use hf_application::workflow::identity::definition_sha256;
use hf_application::workflow::snapshot::{launch_definition, prepare_snapshot};
use hf_domain::workflow::json::Json;
use workflow_parity_support::RealCodec;
use workflow_snapshot_support::{entry, tree, validator};

const CASE_IDS: [&str; 4] = [
    "backend_change_drops_six",
    "same_backend_retains_six",
    "absent_overrides",
    "empty_overrides",
];

fn snapshot_cases() -> Vec<workflow_parity_support::Case> {
    let cases = workflow_parity_support::load_cases("snapshot.json");
    let mut ids: Vec<&str> = cases.iter().map(|case| case.id.as_str()).collect();
    ids.sort_unstable();
    let mut expected = CASE_IDS.to_vec();
    expected.sort_unstable();
    assert_eq!(ids, expected, "snapshot corpus cases changed");
    cases
}

#[test]
fn launch_and_digest_match_the_pure_python_oracle_corpus() {
    for case in snapshot_cases() {
        let overrides = match case.get("overrides") {
            Json::Object(value) => Some(value),
            Json::Null => None,
            other => panic!("{} has invalid override corpus value {other:?}", case.id),
        };
        let Json::Object(input) = case.input() else {
            panic!("{} input is not an object", case.id)
        };
        let launched = launch_definition(&validator(), input, overrides).unwrap();
        assert_eq!(
            Json::Object(launched.clone()),
            *case.get("expected"),
            "{} launch value/order",
            case.id
        );
        assert_eq!(
            definition_sha256(&RealCodec, &launched),
            case.get("definition_sha256").as_str().unwrap(),
            "{} original hash",
            case.id
        );
    }
}

#[test]
fn prepared_pin_rehashes_match_independent_python_hashes() {
    for case in snapshot_cases() {
        let overrides = match case.get("overrides") {
            Json::Object(value) => Some(value),
            Json::Null => None,
            other => panic!("{} has invalid override corpus value {other:?}", case.id),
        };
        let Json::Object(input) = case.input() else {
            panic!("{} input is not an object", case.id)
        };
        let Json::Object(launched) = case.get("expected") else {
            panic!("{} expected launch is not an object", case.id)
        };
        let Json::Object(pin_definition) = case.get("pin_definition") else {
            panic!("{} pin is not an object", case.id)
        };
        let root_hash = case.get("definition_sha256").as_str().unwrap();
        let pin_hash = case.get("pin_definition_sha256").as_str().unwrap();
        let pins = tree(
            "root-id",
            vec![
                entry("root-id", "snapshot launch", launched.clone(), root_hash),
                entry(
                    "child-id",
                    "snapshot launch",
                    pin_definition.clone(),
                    pin_hash,
                ),
            ],
            vec![],
        );
        let snapshot = prepare_snapshot(
            &validator(),
            &RealCodec,
            "snapshot launch",
            input,
            overrides,
            &pins,
        )
        .unwrap();
        assert_eq!(
            snapshot.dependency_tree.workflows[0].1.definition_sha256,
            root_hash
        );
        assert_eq!(
            snapshot.dependency_tree.workflows[1].1.definition_sha256,
            case.get("pin_forced_definition_sha256").as_str().unwrap()
        );
    }
}
