#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The seed tool writes with the Suppressed checkpoint policy: a mutating operation only
//! appends to `-wal` and never copies frames into the main file.
mod support;
use std::{fs, path::Path};
use support::{digest, ok, seeded, serial, task};

const FRESH: &str = "ffffffff-ffff-4fff-8fff-ffffffffffff";

fn files(root: &Path) -> (Vec<u8>, Vec<u8>) {
    let wal = root.join("store.sqlite3-wal");
    assert!(fs::symlink_metadata(&wal).unwrap().file_type().is_file());
    (
        fs::read(root.join("store.sqlite3")).unwrap(),
        fs::read(wal).unwrap(),
    )
}

#[test]
fn a_mutating_seed_operation_appends_to_the_wal_and_never_checkpoints() {
    let _serial = serial();
    let scratch = seeded();
    let (main, wal) = files(&scratch.root);
    assert!(!wal.is_empty(), "the seeded root keeps its log");

    assert_eq!(
        ok(
            &scratch.root,
            "add_task",
            &format!("{{\"spec\":{}}}", task(FRESH, &[]))
        ),
        "{}\n"
    );
    let (after_main, after_wal) = files(&scratch.root);
    assert_eq!(after_main, main, "no frame was copied into the main file");
    assert!(after_wal.len() > wal.len(), "the commit was appended");
    assert_eq!(
        &after_wal[..wal.len()],
        wal.as_slice(),
        "earlier frames kept"
    );
}

#[test]
fn reading_operations_leave_the_seeded_files_byte_identical() {
    let _serial = serial();
    let scratch = seeded();
    let before = files(&scratch.root);
    let first = digest(&scratch.root);
    assert_eq!(digest(&scratch.root), first);
    assert_eq!(files(&scratch.root), before);
}
