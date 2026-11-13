#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The verification files are written through `<name>.tmp`: a stale temporary file
//! is replaced, a planted link is never followed, and the result is a regular file.
mod common;
mod read_session_support;
mod verify_support;
use hf_storage::Verification;
use read_session_support::{later, seeded};
use std::{fs, os::unix::fs::symlink, path::Path};
use verify_support::{
    FENCE, PERIOD, VERIFY, at, damaged, due_bytes, exists, fence_bytes, generation, open_store,
    verification_files,
};

const NOW: u64 = 1_800_000_000;
const DECOY: &[u8] = b"decoy contents\n";

fn regular(root: &Path, name: &str) -> bool {
    fs::symlink_metadata(root.join(name)).unwrap().is_file()
}

#[test]
fn a_link_planted_at_the_due_temporary_is_not_followed() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let target = seeded.scratch.path().join("decoy");
    let store = open_store(root).unwrap();
    for label in ["existing target", "dangling target"] {
        if label == "existing target" {
            fs::write(&target, DECOY).unwrap();
        } else {
            fs::remove_file(&target).unwrap();
        }
        symlink(&target, root.join(format!("{VERIFY}.tmp"))).unwrap();
        assert_eq!(
            store.verify_if_due(at(NOW), later(30)).unwrap(),
            Some(Verification::Passed),
            "{label}"
        );
        assert!(regular(root, VERIFY), "{label}");
        assert_eq!(
            fs::read(root.join(VERIFY)).unwrap(),
            due_bytes(&generation(root), &(NOW + PERIOD).to_string()),
            "{label}"
        );
        assert_eq!(
            verification_files(root),
            vec![VERIFY.to_string()],
            "{label}"
        );
        fs::remove_file(root.join(VERIFY)).unwrap();
    }
    assert!(
        !target.exists(),
        "a dangling link must not be created through"
    );
}

#[test]
fn a_stale_regular_temporary_is_replaced() {
    let seeded = seeded(3);
    let root = &seeded.root;
    fs::write(root.join(format!("{VERIFY}.tmp")), b"left over by a crash").unwrap();
    let store = open_store(root).unwrap();
    assert_eq!(
        store.verify_if_due(at(NOW), later(30)).unwrap(),
        Some(Verification::Passed)
    );
    assert_eq!(verification_files(root), vec![VERIFY.to_string()]);
}

#[test]
fn a_directory_at_the_due_temporary_fails_the_record_and_writes_nothing() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let temporary = root.join(format!("{VERIFY}.tmp"));
    fs::create_dir(&temporary).unwrap();
    fs::write(temporary.join("inside"), b"kept").unwrap();
    let store = open_store(root).unwrap();
    let outcome = store.verify_if_due(at(NOW), later(30));
    assert!(outcome.is_err(), "{outcome:?}");
    assert!(!exists(root, VERIFY));
    assert!(!exists(root, FENCE));
    assert_eq!(fs::read(temporary.join("inside")).unwrap(), b"kept");
    // Still due: an unrecorded pass only repeats the check.
    assert!(store.verification_due(at(NOW)).unwrap());
}

#[test]
fn a_link_planted_at_the_fence_temporary_is_not_followed() {
    let seeded = damaged();
    let root = &seeded.root;
    let target = seeded.scratch.path().join("decoy");
    fs::write(&target, DECOY).unwrap();
    symlink(&target, root.join(format!("{FENCE}.tmp"))).unwrap();
    let store = open_store(root).unwrap();
    assert_eq!(
        store.verify_in_place(later(30)).unwrap(),
        Verification::Fenced
    );
    assert!(regular(root, FENCE));
    assert_eq!(
        fs::read(root.join(FENCE)).unwrap(),
        fence_bytes(&generation(root), "failed")
    );
    assert_eq!(fs::read(&target).unwrap(), DECOY);
    assert_eq!(verification_files(root), vec![FENCE.to_string()]);
    assert!(!exists(root, VERIFY), "a failed check writes no due record");
}
