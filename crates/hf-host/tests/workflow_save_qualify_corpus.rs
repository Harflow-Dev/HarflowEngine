//! Save-time stored qualification must match validation across every accepted corpus case.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_parity_support;
#[path = "../../hf-application/tests/workflow_save_support/mod.rs"]
mod workflow_save_support;

use hf_application::workflow::catalog::CatalogFailure;
use hf_application::workflow::ports::{AuthorityRefusal, DefinitionStore, SaveAuthority};
use hf_application::workflow::save::{DefinitionSaver, SaveExpectations};
use hf_application::workflow::validate::{Validator, WorkflowFailure};
use hf_domain::workflow::document::qualify_stored;
use hf_domain::workflow::json::{Json, JsonInt, JsonObject};
use hf_domain::workflow::refusal::DocumentReason;
use hf_domain::workflow::tree::DependencyTree;
use hf_interfaces::workflow::messages_catalog::catalog_failure_message;
use workflow_parity_support::{RealCodec, load_cases, resolver_entries};
use workflow_save_support::workflow_support::{FixedClock, MapResolver, SeqIds, TableCatalog};
use workflow_save_support::{MemStore, definition, save};

struct Allow;

impl SaveAuthority for Allow {
    fn admit(&self, _: &DependencyTree) -> Result<(), AuthorityRefusal> {
        Ok(())
    }
}

struct NaNClock;

impl hf_application::workflow::ports::Clock for NaNClock {
    fn unix_seconds(&self) -> f64 {
        f64::NAN
    }
}

fn name(definition: &JsonObject) -> &str {
    definition.get("name").and_then(Json::as_str).unwrap_or("")
}

fn reference_ids(definition: &JsonObject) -> Vec<String> {
    let Some(Json::Array(nodes)) = definition.get("nodes") else {
        return Vec::new();
    };
    nodes
        .iter()
        .filter_map(Json::as_object)
        .filter_map(|node| node.get("workflow_ref"))
        .filter_map(Json::as_object)
        .filter_map(|reference| reference.get("workflow_id"))
        .filter_map(Json::as_str)
        .map(str::to_string)
        .collect()
}

fn final_view(definition: &JsonObject) -> JsonObject {
    let mut view = definition.clone();
    view.remove("workflow_id");
    view.remove("revision");
    view.remove("updated_at");
    view.insert("execution_contract", Json::text("delegation"));
    view.insert("context_delivery", Json::text("optimized_v1"));
    view
}

fn candidate_store(candidate: &JsonObject) -> MemStore {
    let store = MemStore::new();
    let ids = SeqIds::default();
    for (index, target_id) in reference_ids(candidate)
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .enumerate()
    {
        let dependency_name = format!("fixture-dependency-{index}");
        let mut dependency = save(
            &store,
            &definition(&dependency_name),
            &SaveExpectations::default(),
            &Allow,
            &ids,
        )
        .unwrap();
        dependency.insert("workflow_id", Json::text(&target_id));
        store.insert(&dependency_name, dependency);
    }
    store
}

#[test]
fn save_qualification_matches_validate_for_each_valid_corpus_case() {
    let catalog = TableCatalog;
    let resolver = MapResolver::new(resolver_entries());
    let validator = Validator::new(&catalog, &resolver);
    let codec = RealCodec;
    let ids = SeqIds::default();
    let clock = FixedClock(1234.5);
    let authority = Allow;
    let cases = load_cases("validate.json");
    let mut checked = 0;
    let mut failures = Vec::new();

    for case in cases {
        if case.expected().get("ok") != Some(&Json::Bool(true)) {
            continue;
        }
        let Json::Object(input) = case.input() else {
            continue;
        };
        let name = name(input);
        let validated = validator.validate(case.input());
        let validation_qualification = match validated {
            Ok(normalized) => qualify_stored(&final_view(&normalized), None),
            Err(WorkflowFailure::Qualification(reason)) => Err(reason),
            Err(other) => {
                failures.push(format!("{}: validate refused {other:?}", case.id));
                continue;
            }
        };
        let store = candidate_store(input);
        let saved = DefinitionSaver {
            validator: &validator,
            codec: &codec,
            ids: &ids,
            clock: &clock,
            authority: &authority,
        }
        .save(&store, name, input, &SaveExpectations::default());
        match saved {
            Ok(final_document) => {
                checked += 1;
                let save_qualification = qualify_stored(&final_document, Some(name));
                if save_qualification != validation_qualification {
                    failures.push(format!(
                        "{}: validate={validation_qualification:?}, save={save_qualification:?}",
                        case.id
                    ));
                }
            }
            Err(error) => failures.push(format!("{}: save refused {error:?}", case.id)),
        }
    }

    assert!(
        checked > 100,
        "only {checked} valid cases reached successful save"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn nan_clock_has_bare_qualification_message_and_writes_nothing() {
    let catalog = TableCatalog;
    let resolver = MapResolver::identity(&[]);
    let validator = Validator::new(&catalog, &resolver);
    let codec = RealCodec;
    let ids = SeqIds::default();
    let authority = Allow;
    let nan_store = MemStore::new();
    let nan = DefinitionSaver {
        validator: &validator,
        codec: &codec,
        ids: &ids,
        clock: &NaNClock,
        authority: &authority,
    }
    .save(
        &nan_store,
        "nan-clock",
        &definition("nan-clock"),
        &SaveExpectations::default(),
    )
    .unwrap_err();
    assert_eq!(
        nan,
        CatalogFailure::Validation(WorkflowFailure::Qualification(
            DocumentReason::NonFiniteValue
        ))
    );
    assert_eq!(
        catalog_failure_message(&nan),
        "Definition must contain finite JSON values"
    );
    assert!(nan_store.calls().is_empty());
}

#[test]
fn revision_digit_carry_has_bare_reader_message_and_writes_nothing() {
    let catalog = TableCatalog;
    let resolver = MapResolver::identity(&[]);
    let validator = Validator::new(&catalog, &resolver);
    let codec = RealCodec;
    let ids = SeqIds::default();
    let authority = Allow;
    let carry_store = MemStore::new();
    let mut existing = DefinitionSaver {
        validator: &validator,
        codec: &codec,
        ids: &ids,
        clock: &FixedClock(1234.5),
        authority: &authority,
    }
    .save(
        &carry_store,
        "carry",
        &definition("carry"),
        &SaveExpectations::default(),
    )
    .unwrap();
    let workflow_id = existing
        .get("workflow_id")
        .and_then(Json::as_str)
        .unwrap()
        .to_string();
    let current = JsonInt::parse(false, &"9".repeat(4300)).unwrap();
    existing.insert("revision", Json::Int(current.clone()));
    carry_store.insert("carry", existing.clone());
    let calls_before = carry_store.calls();
    let error = DefinitionSaver {
        validator: &validator,
        codec: &codec,
        ids: &ids,
        clock: &FixedClock(1234.5),
        authority: &authority,
    }
    .save(
        &carry_store,
        "carry",
        &definition("carry"),
        &SaveExpectations {
            revision: Some(current),
            workflow_id: Some(workflow_id),
        },
    )
    .unwrap_err();
    assert_eq!(
        error,
        CatalogFailure::Validation(WorkflowFailure::Qualification(
            DocumentReason::IntegerTooLong { digits: 4301 }
        ))
    );
    assert_eq!(
        catalog_failure_message(&error),
        "Exceeds the limit (4300 digits) for integer string conversion: value has 4301 digits; use sys.set_int_max_str_digits() to increase the limit",
        "reader qualification must render without a source-name prefix"
    );
    assert_eq!(carry_store.calls(), calls_before);
    assert_eq!(
        carry_store.get("carry").unwrap().unwrap().definition,
        existing
    );
}
