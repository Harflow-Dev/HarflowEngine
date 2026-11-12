#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The single commit path keeps each site's task-list publication, and a root without a
//! usable `commit_sequence` row is refused before any mutation.
mod common;
mod fixture_support;
use common::{Fixture, deadline, default_limits, snapshot_tree};
use fixture_support::{A, B, World, artifact_item, done, event, later, tid, world};
use hf_domain::task_read::Coverage;
use hf_storage::{ChainStop, Error, LaunchRow, Mutation, Store, TaskFixture};
use rusqlite::Connection;
use std::{fs, path::Path};

fn state(w: &World) -> (String, i64) {
    (w.generation(), w.watch())
}

fn empty() -> TaskFixture {
    TaskFixture {
        rows: Vec::new(),
        launches: Vec::new(),
        coverage: Coverage {
            discovery_complete: true,
            ownership_complete: true,
            source_complete: true,
        },
    }
}

/// What one write does to (dataset generation, watch revision).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Effect {
    GenerationAndRevision,
    Revision,
    Generation,
    Nothing,
}

fn assert_effect(w: &World, what: &str, expected: Effect, write: impl FnOnce(&Store)) {
    let (generation, revision) = state(w);
    write(&w.store);
    let (new_generation, new_revision) = state(w);
    let observed = match (new_generation != generation, new_revision - revision) {
        (true, 1) => Effect::GenerationAndRevision,
        (false, 1) => Effect::Revision,
        (true, 0) => Effect::Generation,
        (false, 0) => Effect::Nothing,
        other => panic!("{what}: unexpected change {other:?}"),
    };
    assert_eq!(observed, expected, "{what}");
}

#[test]
fn task_and_launch_sites_keep_their_generation_and_revision_effects() {
    let w = world();
    assert_effect(
        &w,
        "replace_task_fixture",
        Effect::GenerationAndRevision,
        |s| s.replace_task_fixture(&empty(), later(5)).unwrap(),
    );
    assert_effect(&w, "add_task_fixture", Effect::GenerationAndRevision, |s| {
        s.add_task_fixture(&done(A), later(5)).unwrap()
    });
    assert_effect(
        &w,
        "damage_task_fixture",
        Effect::GenerationAndRevision,
        |s| s.damage_task_fixture(&tid(A), later(5)).unwrap(),
    );
    assert_effect(&w, "put_launch_attempt", Effect::Generation, |s| {
        s.put_launch_attempt(&LaunchRow::waiting_for_resources(tid(B)), later(5))
            .unwrap()
    });
    assert_effect(&w, "replace_launch_fixture", Effect::Generation, |s| {
        s.replace_launch_fixture(&[LaunchRow::waiting_for_resources(tid(B))], later(5))
            .unwrap()
    });
}

#[test]
fn sites_that_publish_nothing_change_neither_generation_nor_revision() {
    let w = world();
    w.add(&done(A));
    assert_effect(&w, "append_event_record_fixture", Effect::Nothing, |s| {
        s.append_event_record_fixture(&tid(A), &event(0, "task_started"), later(5))
            .unwrap()
    });
    assert_effect(&w, "truncate_event_log_fixture", Effect::Nothing, |s| {
        s.truncate_event_log_fixture(&tid(A), 0, later(5)).unwrap()
    });
    assert_effect(&w, "put_chain_stop", Effect::Nothing, |s| {
        s.put_chain_stop(&tid(A), &ChainStop::Fault, later(5))
            .unwrap()
    });
    assert_effect(&w, "remove_chain_stop", Effect::Nothing, |s| {
        s.remove_chain_stop(&tid(A), later(5)).unwrap()
    });
    assert_effect(&w, "replace_artifacts_fixture", Effect::Nothing, |s| {
        s.replace_artifacts_fixture(&tid(A), true, &[artifact_item("a1", "/p")], later(5))
            .unwrap()
    });
    assert_effect(&w, "apply_batch", Effect::Nothing, |s| {
        s.apply_batch(
            &[Mutation::Put {
                key: "k".into(),
                value: vec![1],
            }],
            later(5),
        )
        .unwrap()
    });
}

#[test]
fn a_refused_publishing_write_changes_neither_generation_nor_revision() {
    let w = world();
    w.add(&done(A));
    assert_effect(&w, "duplicate add", Effect::Nothing, |s| {
        assert!(matches!(
            s.add_task_fixture(&done(A), later(5)),
            Err(Error::FixtureConflict)
        ));
    });
}

fn tamper(root: &Path, statement: &str) {
    let raw = Connection::open(root.join("store.sqlite3")).unwrap();
    raw.execute_batch(statement).unwrap();
}

/// Durable bytes only: the `-shm` file is SQLite's volatile shared index and an empty `-wal`
/// is what any WAL-mode connection leaves behind; neither carries committed data.
fn durable(root: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    snapshot_tree(root)
        .into_iter()
        .filter(|(path, bytes)| {
            let name = path.to_string_lossy();
            !(name.ends_with("-shm") || (name.ends_with("-wal") && bytes.is_empty()))
        })
        .collect()
}

fn assert_refused_before_mutation(root: &Path, scratch: &Path, label: &str) {
    let before = durable(root);
    let opened = Store::open(
        root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    );
    assert!(matches!(opened, Err(Error::Incompatible)), "{label}: open");
    assert_eq!(durable(root), before, "{label}: open left bytes");
    let recovered = Store::recover(root, deadline(), scratch, common::lease::lease_port());
    assert!(
        matches!(recovered, Err(Error::Incompatible)),
        "{label}: recover {recovered:?}"
    );
    assert_eq!(durable(root), before, "{label}: recover left bytes");
}

#[test]
fn a_root_without_a_usable_sequence_row_is_incompatible_for_open_and_recover() {
    for (label, statement) in [
        ("missing table", "DROP TABLE commit_sequence"),
        ("missing row", "DELETE FROM commit_sequence"),
        (
            "non-integer value",
            "UPDATE commit_sequence SET value='zero'",
        ),
    ] {
        let fixture = Fixture::new();
        let root = fixture.path().join("store");
        let scratch = fixture.path().join("scratch");
        fs::create_dir(&scratch).unwrap();
        drop(
            Store::initialize_fixture(
                &root,
                default_limits(),
                deadline(),
                common::lease::lease_port(),
            )
            .unwrap(),
        );
        tamper(&root, statement);
        assert_refused_before_mutation(&root, &scratch, label);
    }
}

#[test]
fn an_intact_root_still_opens_and_recovers() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let scratch = fixture.path().join("scratch");
    fs::create_dir(&scratch).unwrap();
    drop(
        Store::initialize_fixture(
            &root,
            default_limits(),
            deadline(),
            common::lease::lease_port(),
        )
        .unwrap(),
    );
    drop(
        Store::open(
            &root,
            default_limits(),
            deadline(),
            common::lease::lease_port(),
        )
        .unwrap(),
    );
    Store::recover(&root, deadline(), &scratch, common::lease::lease_port()).unwrap();
}
