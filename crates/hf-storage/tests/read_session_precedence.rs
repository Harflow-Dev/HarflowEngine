#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Which refusal wins when a defective `-wal` meets another defect, and what a
//! defective sidecar or marker is reported as.
mod common;
mod read_session_support;
use hf_storage::{Error, ReadRefusal, SCHEMA_VERSION};
use read_session_support::{MAIN, SHM, Seeded, WAL, checkpoint, edit_main, open, read_all, seeded};
use std::{fs, path::Path};

fn refusal(root: &Path) -> ReadRefusal {
    match open(root) {
        Ok(_) => panic!("the root must be refused"),
        Err(error) => ReadRefusal::from(&error),
    }
}

/// Replaces the `-wal` with a directory or a symlink.
fn break_wal(seeded: &Seeded, kind: &str) {
    let wal = seeded.root.join(WAL);
    fs::remove_file(&wal).unwrap();
    match kind {
        "missing" => {}
        "directory" => fs::create_dir(&wal).unwrap(),
        _ => {
            let target = seeded.scratch.path().join("elsewhere-wal");
            fs::write(&target, b"").unwrap();
            std::os::unix::fs::symlink(&target, &wal).unwrap();
        }
    }
}

fn write_marker(seeded: &Seeded, schema: u32) {
    let marker = fs::read_to_string(seeded.root.join("store.identity")).unwrap();
    let generation = marker.lines().nth(2).unwrap().to_owned();
    fs::write(
        seeded.root.join("store.identity"),
        format!("harflow-storage-2\n{schema}\n{generation}\n"),
    )
    .unwrap();
}

const BROKEN: [&str; 3] = ["missing", "directory", "symlink"];

#[test]
fn baseline_an_intact_root_opens_so_each_case_below_tests_one_defect() {
    let seeded = seeded(5);
    assert!(open(&seeded.root).is_ok());
}

#[test]
fn a_defective_wal_does_not_mask_a_newer_marker_or_database() {
    for kind in BROKEN {
        let seeded = seeded(5);
        write_marker(&seeded, SCHEMA_VERSION + 1);
        break_wal(&seeded, kind);
        assert_eq!(
            refusal(&seeded.root),
            ReadRefusal::FutureSchema,
            "marker, {kind}"
        );
        let seeded = seeded_with_newer_database();
        break_wal(&seeded, kind);
        assert_eq!(
            refusal(&seeded.root),
            ReadRefusal::FutureSchema,
            "database, {kind}"
        );
    }
}

fn seeded_with_newer_database() -> Seeded {
    let seeded = seeded(5);
    edit_main(
        &seeded.root,
        &format!("PRAGMA user_version={};", SCHEMA_VERSION + 1),
    );
    seeded
}

#[test]
fn a_defective_wal_does_not_mask_a_foreign_or_older_or_malformed_marker() {
    for kind in BROKEN {
        let seeded = seeded(5);
        checkpoint(&seeded.root);
        let mut main = fs::read(seeded.root.join(MAIN)).unwrap();
        main[68..72].copy_from_slice(&1u32.to_be_bytes());
        fs::write(seeded.root.join(MAIN), main).unwrap();
        break_wal(&seeded, kind);
        assert_eq!(
            refusal(&seeded.root),
            ReadRefusal::ForeignStore,
            "foreign, {kind}"
        );

        let seeded = seeded_older();
        break_wal(&seeded, kind);
        assert_eq!(
            refusal(&seeded.root),
            ReadRefusal::UnsupportedLayout,
            "older, {kind}"
        );

        let seeded = self::seeded(5);
        fs::write(seeded.root.join("store.identity"), b"not a marker\n").unwrap();
        break_wal(&seeded, kind);
        assert_eq!(
            refusal(&seeded.root),
            ReadRefusal::InvalidMarker,
            "malformed, {kind}"
        );
    }
}

fn seeded_older() -> Seeded {
    let seeded = seeded(5);
    write_marker(&seeded, SCHEMA_VERSION - 1);
    seeded
}

#[test]
fn a_defective_wal_does_not_mask_a_nonempty_journal() {
    for kind in BROKEN {
        let seeded = seeded(5);
        fs::write(seeded.root.join("store.sqlite3-journal"), b"pending").unwrap();
        break_wal(&seeded, kind);
        assert_eq!(
            refusal(&seeded.root),
            ReadRefusal::NonemptyJournal,
            "{kind}"
        );
    }
}

#[test]
fn a_defective_wal_alone_is_a_missing_wal() {
    for kind in BROKEN {
        let seeded = seeded(5);
        break_wal(&seeded, kind);
        assert_eq!(refusal(&seeded.root), ReadRefusal::WalMissing, "{kind}");
    }
}

#[test]
fn a_shm_that_is_a_directory_is_refused_but_is_not_a_missing_wal() {
    let seeded = seeded(5);
    let _ = fs::remove_file(seeded.root.join(SHM));
    fs::create_dir(seeded.root.join(SHM)).unwrap();
    let outcome = open(&seeded.root);
    let error = outcome.err().expect("a directory -shm cannot be used");
    assert!(!matches!(error, Error::WalMissing), "{error:?}");
    assert_ne!(ReadRefusal::from(&error), ReadRefusal::WalMissing);
    assert!(seeded.root.join(WAL).is_file());
}

#[test]
fn a_wal_that_becomes_non_regular_after_open_refuses_the_next_read() {
    for kind in ["directory", "symlink"] {
        let seeded = seeded(5);
        let session = open(&seeded.root).unwrap();
        assert_eq!(read_all(&session).unwrap().len(), 5);
        break_wal(&seeded, kind);
        let error = read_all(&session).err().unwrap();
        assert!(matches!(error, Error::WalMissing), "{kind}: {error:?}");
    }
}

#[test]
fn a_marker_that_is_not_utf8_is_an_invalid_marker() {
    let seeded = seeded(5);
    let marker = fs::read(seeded.root.join("store.identity")).unwrap();
    let mut broken = marker.clone();
    broken.splice(18..19, [0xff, 0xfe]);
    assert_ne!(marker, broken);
    fs::write(seeded.root.join("store.identity"), broken).unwrap();
    assert_eq!(refusal(&seeded.root), ReadRefusal::InvalidMarker);
}

#[test]
fn a_missing_or_non_regular_marker_or_database_is_an_invalid_marker() {
    for name in ["store.identity", MAIN] {
        for kind in ["missing", "directory"] {
            let seeded = seeded(5);
            let path = seeded.root.join(name);
            fs::remove_file(&path).unwrap();
            if kind == "directory" {
                fs::create_dir(&path).unwrap();
            }
            assert_eq!(
                refusal(&seeded.root),
                ReadRefusal::InvalidMarker,
                "{name} {kind}"
            );
        }
    }
}

// A poisoned connection mutex (the path that must not report an invalid marker) needs a
// panic inside a read action while the guard is held; no public seam can cause one, so
// it is not exercised here.
