//! Workflow identity: the stored id, the name-derived legacy id and the persisted id.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_support;

use hf_application::workflow::identity::{
    definition_sha256, persist_workflow_id, workflow_identity,
};
use hf_application::workflow::ports::{DefinitionCodec, IdSource};
use hf_domain::workflow::json::{Json, JsonObject};
use workflow_support::{FakeCodec, SeqIds};

fn object(entries: &[(&str, Json)]) -> JsonObject {
    JsonObject::from_entries(
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
    )
}

fn legacy(name: &str) -> String {
    format!("legacy-{}", &FakeCodec.sha256_hex(name.as_bytes())[..32])
}

#[test]
fn stored_id_is_kept() {
    let definition = object(&[
        ("name", Json::text("flow")),
        ("workflow_id", Json::text("abc123")),
    ]);
    assert_eq!(workflow_identity(&FakeCodec, &definition), "abc123");
}

#[test]
fn missing_or_unusable_id_falls_back_to_the_name() {
    let id = |value: Option<Json>| {
        let mut entries = vec![("name", Json::text("flow"))];
        if let Some(value) = value {
            entries.push(("workflow_id", value));
        }
        workflow_identity(&FakeCodec, &object(&entries))
    };
    for unusable in [
        None,
        Some(Json::text("")),
        Some(Json::text("  ")),
        Some(Json::text("\u{a0}")),
        Some(Json::int(7)),
        Some(Json::Null),
    ] {
        assert_eq!(id(unusable.clone()), legacy("flow"), "{unusable:?}");
    }
    assert_eq!(legacy("flow").len(), "legacy-".len() + 32);
}

#[test]
fn legacy_id_uses_python_str_of_the_name() {
    let id = |name: Json| workflow_identity(&FakeCodec, &object(&[("name", name)]));
    assert_eq!(id(Json::int(5)), legacy("5"));
    assert_eq!(id(Json::Bool(true)), legacy("True"));
    assert_eq!(id(Json::Bool(false)), legacy("False"));
    assert_eq!(id(Json::Null), legacy("None"));
    assert_eq!(
        workflow_identity(&FakeCodec, &JsonObject::new()),
        legacy("")
    );
}

#[test]
fn different_names_give_different_legacy_ids() {
    let id = |name: &str| workflow_identity(&FakeCodec, &object(&[("name", Json::text(name))]));
    assert_ne!(id("one"), id("two"));
    assert_eq!(id("one"), id("one"));
}

#[test]
fn definition_digest_comes_from_the_codec() {
    let definition = object(&[("name", Json::text("flow")), ("n", Json::int(1))]);
    assert_eq!(
        definition_sha256(&FakeCodec, &definition),
        FakeCodec.definition_sha256(&definition)
    );
    let reordered = object(&[("n", Json::int(1)), ("name", Json::text("flow"))]);
    assert_eq!(
        definition_sha256(&FakeCodec, &definition),
        definition_sha256(&FakeCodec, &reordered)
    );
    let other = object(&[("name", Json::text("flow")), ("n", Json::int(2))]);
    assert_ne!(
        definition_sha256(&FakeCodec, &definition),
        definition_sha256(&FakeCodec, &other)
    );
}

#[test]
fn persisted_id_prefers_the_previous_one() {
    let ids = SeqIds::default();
    let previous = object(&[
        ("name", Json::text("flow")),
        ("workflow_id", Json::text("kept")),
    ]);
    assert_eq!(
        persist_workflow_id(&FakeCodec, &ids, Some(&previous)),
        "kept"
    );
    assert_eq!(
        ids.workflow_id(),
        format!("{:032x}", 1),
        "no id was minted for an existing definition"
    );
}

#[test]
fn persisted_id_for_a_legacy_definition_is_name_derived() {
    let ids = SeqIds::default();
    for stored in [
        None,
        Some(Json::text("")),
        Some(Json::int(3)),
        Some(Json::Null),
    ] {
        let mut entries = vec![("name", Json::text("flow"))];
        if let Some(value) = stored.clone() {
            entries.push(("workflow_id", value));
        }
        let previous = object(&entries);
        assert_eq!(
            persist_workflow_id(&FakeCodec, &ids, Some(&previous)),
            legacy("flow"),
            "{stored:?}"
        );
    }
}

#[test]
fn persisted_id_for_a_new_definition_is_minted() {
    let ids = SeqIds::default();
    let first = persist_workflow_id(&FakeCodec, &ids, None);
    let second = persist_workflow_id(&FakeCodec, &ids, None);
    assert_eq!(first, format!("{:032x}", 1));
    assert_eq!(second, format!("{:032x}", 2));
    assert_eq!(first.len(), 32);
}
