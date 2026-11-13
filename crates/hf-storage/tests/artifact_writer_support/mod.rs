#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Shared helpers for the production artifact writer tests: an item builder, the codec
//! lengths of the reference manifest, and the state a registration may change.

use crate::fixture_support::{World, later, one, tid};
use crate::task_writer_support::published;
use hf_domain::ctl::history::{ArtifactItem, Observation, PlanDirectory, PlanEvidence};
use hf_storage::{ManifestLengths, Registered, Registrant};
type Result<T> = hf_storage::Result<T>;

/// `json.dumps(..., ensure_ascii=False)` length of the empty manifest of task `A`
/// (`fixture_support::A`), product version `0.0.13+rust`; computed with Python and checked
/// against the interface codec in `hf-interfaces/tests/artifact_codec.rs`.
pub const ENVELOPE_A: u64 = 164;
/// The reference length of `item(..)` registered by a caller to task `A` (Python, as above).
pub const ITEM_A: u64 = 373;
pub const MANIFEST_MAX: u64 = 1_048_576;

pub const ITEM_COLUMNS: &str = "ordinal,artifact_id,path,kind,note,registered_at,provenance_method,device,inode,size,mtime_ns,tool_use_id,plan_directory,plan_device,plan_inode,encoded_len";

pub fn observation(inode: u64) -> Observation {
    Observation {
        device: 1,
        inode,
        size: 3,
        mtime_ns: 4,
    }
}

pub fn item(artifact_id: &str, path: &str) -> ArtifactItem {
    ArtifactItem {
        artifact_id: artifact_id.into(),
        path: path.into(),
        kind: "report".into(),
        note: "n".into(),
        registered_at: "2026-01-10T10:04:00+00:00".into(),
        method: "verified_process_lineage".into(),
        registered: observation(2),
        plan: None,
    }
}

pub fn engine_item(artifact_id: &str, path: &str) -> ArtifactItem {
    ArtifactItem {
        method: "engine_output".into(),
        ..item(artifact_id, path)
    }
}

pub fn plan_item(artifact_id: &str, directory: Option<(&str, u64, u64)>) -> ArtifactItem {
    ArtifactItem {
        method: "observed_backend_plan".into(),
        plan: Some(PlanEvidence {
            tool_use_id: "toolu_01".into(),
            directory: directory.map(|(path, device, inode)| PlanDirectory {
                path: path.into(),
                device,
                inode,
            }),
        }),
        ..item(artifact_id, "/p/plan")
    }
}

pub fn lengths(envelope: u64, item: u64) -> ManifestLengths {
    ManifestLengths { envelope, item }
}

pub fn usual() -> ManifestLengths {
    lengths(ENVELOPE_A, ITEM_A)
}

pub fn register(
    w: &World,
    id: &str,
    item: &ArtifactItem,
    lengths: ManifestLengths,
    registrant: Registrant,
) -> Result<Registered> {
    w.store
        .register_artifact(&tid(id), item, lengths, registrant, later(5))
}

pub fn added(w: &World, id: &str, item: &ArtifactItem, lengths: ManifestLengths, who: Registrant) {
    assert_eq!(
        register(w, id, item, lengths, who).unwrap(),
        Registered::Added,
        "{}",
        item.artifact_id
    );
}

/// The manifest row as `header_valid|revision`, or `NONE`.
pub fn manifest(w: &World, id: &str) -> String {
    let db = w.db();
    if crate::fixture_support::count(&db, "artifact_manifests", "task", id) == 0 {
        return "NONE".to_owned();
    }
    one(
        &db,
        "SELECT header_valid||'|'||revision FROM artifact_manifests WHERE task=?1",
        [id],
    )
}

pub fn revision(w: &World, id: &str) -> String {
    one(
        &w.db(),
        "SELECT revision FROM artifact_manifests WHERE task=?1",
        [id],
    )
}

/// Every stored column of the task's items, in ordinal order, each rendered with `quote()`.
pub fn items(w: &World, id: &str) -> Vec<String> {
    let db = w.db();
    let columns: Vec<String> = ITEM_COLUMNS
        .split(',')
        .map(|c| format!("quote({c})"))
        .collect();
    let mut statement = db
        .prepare(&format!(
            "SELECT {} FROM artifact_items WHERE task=?1 ORDER BY ordinal",
            columns.join("||'|'||")
        ))
        .unwrap();
    statement
        .query_map([id], |r| r.get::<_, String>(0))
        .unwrap()
        .map(std::result::Result::unwrap)
        .collect()
}

/// Everything a refused or deduplicated registration must leave unchanged: the manifest
/// row, the items, the dataset generation, the watch revision and the commit sequence.
pub fn state(w: &World, id: &str) -> (String, Vec<String>, (String, i64, i64)) {
    (manifest(w, id), items(w, id), published(w))
}

/// Asserts `registration` fails with an error `matches` accepts and changes nothing.
pub fn refused(
    w: &World,
    id: &str,
    registration: impl FnOnce() -> Result<Registered>,
    matches: impl Fn(&hf_storage::Error) -> bool,
) {
    let before = state(w, id);
    let error = registration().unwrap_err();
    assert!(matches(&error), "{error:?}");
    assert_eq!(
        state(w, id),
        before,
        "a refusal leaves everything unchanged"
    );
}
