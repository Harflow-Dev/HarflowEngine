#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::support::*;
use crate::{Limits, Store};
use hf_application::task_watch::TaskWatchPort;
use hf_domain::task_watch::{WatchReason, WatchState};
use rusqlite::{Connection, OpenFlags};
use std::fs;
#[test]
fn root_recreation_changes_epoch_and_returns_reset() {
    let fixture = Fixture::new("recreate");
    let store = fixture.initialize();
    let old = watch(&fixture.root(), Clock::default())
        .execute(&request(None, 0), deadline())
        .unwrap()
        .cursor
        .unwrap();
    drop(store);
    fs::remove_dir_all(fixture.root()).unwrap();
    let replacement = fixture.initialize();
    let result = watch(&fixture.root(), Clock::default())
        .execute(&request(Some(old), 1), deadline())
        .unwrap();
    assert_eq!(result.state, WatchState::Reset);
    assert_eq!(result.reason, Some(WatchReason::EpochChanged));
    assert_eq!(read_revision(&fixture.raw()), 0);
    drop(replacement);
}

#[test]
fn schema_two_roots_are_refused_without_upgrade_or_source_rewrite() {
    let fixture = Fixture::new("schema-two");
    let store = fixture.initialize();
    drop(store);
    let raw = Connection::open(fixture.root().join("store.sqlite3")).unwrap();
    raw.pragma_update(None, "user_version", 2).unwrap();
    let before: i64 = raw
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    drop(raw);
    assert_eq!(before, 2);
    assert!(matches!(
        Store::open(
            fixture.root(),
            Limits::default(),
            deadline(),
            crate::tests::lease::lease_port()
        ),
        Err(crate::Error::Incompatible)
    ));
    let verify = fixture.raw();
    let after: i64 = verify
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(
        after, 2,
        "watch/source qualification must not upgrade schema 2"
    );
    let watch_meta: i64 = verify
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='task_watch_meta'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(watch_meta, 1);
}

#[test]
fn empty_unmarked_and_missing_marker_roots_are_refused_without_initialization() {
    let fixture = Fixture::new("invalid-root-types");
    let empty = fixture.root();
    fs::create_dir(&empty).unwrap();
    assert_eq!(
        watch(&empty, Clock::default()).execute(&request(None, 0), deadline()),
        Err(hf_application::task_watch::WatchError::StateIncompatible)
    );
    assert_eq!(fs::read_dir(&empty).unwrap().count(), 0);
    fs::remove_dir_all(&empty).unwrap();

    let unmarked = fixture.0.join("unmarked");
    fs::create_dir(&unmarked).unwrap();
    fs::write(unmarked.join("foreign-note"), b"not a store").unwrap();
    let before = directory_bytes(&unmarked);
    assert_eq!(
        watch(&unmarked, Clock::default()).execute(&request(None, 0), deadline()),
        Err(hf_application::task_watch::WatchError::StateIncompatible)
    );
    assert_eq!(directory_bytes(&unmarked), before);

    let store = fixture.initialize();
    drop(store);
    fs::remove_file(fixture.root().join("store.identity")).unwrap();
    let before = directory_bytes(&fixture.root());
    assert_eq!(
        watch(&fixture.root(), Clock::default()).execute(&request(None, 0), deadline()),
        Err(hf_application::task_watch::WatchError::StateIncompatible)
    );
    assert_eq!(directory_bytes(&fixture.root()), before);
}

fn directory_bytes(root: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut files = fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read(path).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

#[test]
fn uppercase_generation_paired_with_its_marker_is_not_a_cursor_refusal() {
    let fixture = Fixture::new("uppercase-generation");
    let store = fixture.initialize();
    drop(store);
    let raw = Connection::open(fixture.root().join("store.sqlite3")).unwrap();
    raw.execute(
        "UPDATE store_identity SET generation=upper(generation) WHERE singleton=1",
        [],
    )
    .unwrap();
    let generation: String = raw
        .query_row(
            "SELECT generation FROM store_identity WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(raw);
    fs::write(
        fixture.root().join("store.identity"),
        format!("harflow-storage-2\n3\n{generation}\n"),
    )
    .unwrap();
    let before = directory_bytes(&fixture.root());
    assert_eq!(
        watch(&fixture.root(), Clock::default()).execute(&request(None, 0), deadline()),
        Err(hf_application::task_watch::WatchError::StateIncompatible)
    );
    assert_eq!(
        directory_bytes(&fixture.root()),
        before,
        "invalid canonical generation must never be rewritten into cursor-shaped state"
    );
}

#[test]
fn embedded_nul_identity_suffix_is_refused_without_rewriting_database_source() {
    let fixture = Fixture::new("identity-nul");
    let store = fixture.initialize();
    drop(store);
    let raw = Connection::open(fixture.root().join("store.sqlite3")).unwrap();
    raw.execute(
        "UPDATE store_identity SET generation=generation||char(0)||'suffix' WHERE singleton=1",
        [],
    )
    .unwrap();
    let before: (i64, String) = raw.query_row("SELECT length(CAST(generation AS BLOB)), quote(generation) FROM store_identity WHERE singleton=1", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(before.0, 39);
    drop(raw);
    assert!(matches!(
        Store::open(
            fixture.root(),
            Limits::default(),
            deadline(),
            crate::tests::lease::lease_port()
        ),
        Err(crate::Error::Incompatible)
    ));
    let verify = Connection::open_with_flags(
        fixture.root().join("store.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let after: (i64, String) = verify.query_row("SELECT length(CAST(generation AS BLOB)), quote(generation) FROM store_identity WHERE singleton=1", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(
        after, before,
        "strict observation refusal must not rewrite the source identity"
    );
}

#[test]
fn root_replacement_during_wait_is_unavailable_and_fresh_operation_resets() {
    let fixture = Fixture::new("replace-during-wait");
    let old_store = fixture.initialize();
    let old_cursor = watch(&fixture.root(), Clock::default())
        .execute(&request(None, 0), deadline())
        .unwrap()
        .cursor
        .unwrap();
    let retired = fixture.0.join("retired-store");
    drop(old_store);
    // Keep a genuine open watched handle while replacing its selected path in the wait callback.
    let replacement_parent = fixture.root();
    let clock = Clock::default();
    let path = replacement_parent.clone();
    let retired_path = retired.clone();
    clock.on_wait(move || {
        fs::rename(&path, &retired_path).unwrap();
        Store::initialize_fixture(
            &path,
            Limits::default(),
            deadline(),
            crate::tests::lease::lease_port(),
        )
        .unwrap();
    });
    // The callback must run after WatchSession opens the old qualified handle. Its root binding
    // then detects the replacement on the next snapshot rather than reporting Timeout.
    let watch_result = watch(&replacement_parent, clock)
        .execute(&request(Some(old_cursor.clone()), 1), deadline())
        .unwrap();
    assert_eq!(watch_result.state, WatchState::Unavailable);
    assert_eq!(
        watch_result.reason,
        Some(WatchReason::ObservationUnavailable)
    );
    assert_eq!(
        watch(&replacement_parent, Clock::default())
            .execute(&request(Some(old_cursor), 0), deadline())
            .unwrap()
            .state,
        WatchState::Reset
    );
}
