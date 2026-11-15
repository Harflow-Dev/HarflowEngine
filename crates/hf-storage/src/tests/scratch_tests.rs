#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The evidence scratch directory removes only what it created: every recorded identity is
//! verified before each unlink, and a mismatch stops the removal.
use crate::{
    Error,
    maintenance_copy::{Scratch, require_owner},
};
use std::{fs, io::Write, path::PathBuf};

const OP: &str = "0123456789abcdef0123456789abcdef";

fn parent(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("hf-scratch-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir(&path).unwrap();
    let path = fs::canonicalize(path).unwrap();
    fs::create_dir(path.join("store")).unwrap();
    path
}

#[test]
fn a_scratch_with_recorded_files_is_removed() {
    let parent = parent("clean");
    let mut scratch = Scratch::create(&parent.join("store"), OP).unwrap();
    scratch.create_copy().unwrap().write_all(b"x").unwrap();
    fs::write(scratch.database().with_file_name("store.sqlite3-wal"), b"").unwrap();
    scratch.snapshot().unwrap();
    scratch.remove().unwrap();
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn a_replaced_file_stops_removal_and_deletes_nothing_further() {
    let parent = parent("swapped-file");
    let mut scratch = Scratch::create(&parent.join("store"), OP).unwrap();
    scratch.create_copy().unwrap();
    let wal = scratch.database().with_file_name("store.sqlite3-wal");
    fs::write(&wal, b"").unwrap();
    scratch.snapshot().unwrap();
    // Another actor replaces the copy by a different file with the same name.
    let swap = scratch.database().with_file_name("swap");
    fs::write(&swap, b"not ours").unwrap();
    fs::rename(&swap, scratch.database()).unwrap();
    assert!(matches!(scratch.remove(), Err(Error::Replaced)));
    assert_eq!(fs::read(scratch.database()).unwrap(), b"not ours");
    // The other file was not removed before the mismatch stopped the run, or it was
    // removed only because its own identity matched; the replacement stays either way.
    drop(scratch);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn a_replaced_directory_is_not_removed() {
    let parent = parent("swapped-dir");
    let mut scratch = Scratch::create(&parent.join("store"), OP).unwrap();
    let path = scratch.database().parent().unwrap().to_path_buf();
    fs::remove_dir(&path).unwrap();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("store.sqlite3"), b"theirs").unwrap();
    assert!(matches!(scratch.remove(), Err(Error::Replaced)));
    assert_eq!(fs::read(path.join("store.sqlite3")).unwrap(), b"theirs");
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn a_main_file_name_taken_by_another_actor_stops_the_copy_and_survives() {
    let parent = parent("foreign-main");
    let mut scratch = Scratch::create(&parent.join("store"), OP).unwrap();
    fs::write(scratch.database(), b"foreign").unwrap();
    assert!(scratch.create_copy().is_err());
    scratch.snapshot().unwrap();
    assert!(scratch.remove().is_err());
    assert_eq!(fs::read(scratch.database()).unwrap(), b"foreign");
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn a_link_named_like_a_sidecar_is_neither_recorded_nor_deleted() {
    let parent = parent("link-sidecar");
    let mut scratch = Scratch::create(&parent.join("store"), OP).unwrap();
    scratch.create_copy().unwrap();
    let target = parent.join("victim");
    fs::write(&target, b"keep").unwrap();
    let link = scratch.database().with_file_name("store.sqlite3-shm");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    scratch.snapshot().unwrap();
    assert!(scratch.remove().is_err());
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&target).unwrap(), b"keep");
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn a_directory_swapped_for_a_link_between_two_deletions_stops_cleanup() {
    let parent = parent("dir-link");
    let mut scratch = Scratch::create(&parent.join("store"), OP).unwrap();
    scratch.create_copy().unwrap();
    let dir = scratch.database().parent().unwrap().to_path_buf();
    fs::write(dir.join("store.sqlite3-wal"), b"").unwrap();
    scratch.snapshot().unwrap();
    let moved = parent.join("moved");
    let result = scratch.remove_with(&mut |index| {
        if index == 1 {
            fs::rename(&dir, &moved).unwrap();
            std::os::unix::fs::symlink(&moved, &dir).unwrap();
        }
    });
    assert!(matches!(result, Err(Error::Replaced)));
    // Exactly one of the two files was deleted before the swap; the other is untouched.
    assert_eq!(fs::read_dir(&moved).unwrap().count(), 1);
    fs::remove_dir_all(parent).unwrap();
}

fn leftover(parent: &std::path::Path) -> PathBuf {
    let path = parent.join(format!(".store.harflow-evidence-{OP}"));
    fs::create_dir(&path).unwrap();
    path
}

#[test]
fn a_leftover_directory_is_cleared_and_creation_proceeds() {
    let parent = parent("leftover-ok");
    let old = leftover(&parent);
    fs::write(old.join("store.sqlite3"), b"old").unwrap();
    fs::write(old.join("store.sqlite3-wal"), b"").unwrap();
    let mut scratch = Scratch::create(&parent.join("store"), OP).unwrap();
    assert!(!scratch.database().exists());
    scratch.remove().unwrap();
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn a_leftover_swapped_for_a_link_between_listing_and_unlink_deletes_nothing_there() {
    let parent = parent("leftover-link");
    let old = leftover(&parent);
    fs::write(old.join("store.sqlite3-wal"), b"").unwrap();
    let victim = parent.join("victim");
    fs::create_dir(&victim).unwrap();
    fs::write(victim.join("store.sqlite3"), b"live").unwrap();
    let moved = parent.join("moved");
    let result = Scratch::create_with(&parent.join("store"), OP, &mut |_| {
        fs::rename(&old, &moved).unwrap();
        // The same inode is reachable under the victim, so only the directory
        // re-check can stop the unlink.
        fs::hard_link(
            moved.join("store.sqlite3-wal"),
            victim.join("store.sqlite3-wal"),
        )
        .unwrap();
        std::os::unix::fs::symlink(&victim, &old).unwrap();
    });
    assert!(matches!(result, Err(Error::Replaced)));
    assert!(victim.join("store.sqlite3-wal").exists());
    assert_eq!(fs::read(victim.join("store.sqlite3")).unwrap(), b"live");
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn a_leftover_file_swapped_after_listing_is_not_deleted() {
    let parent = parent("leftover-file");
    let old = leftover(&parent);
    fs::write(old.join("store.sqlite3-wal"), b"").unwrap();
    let result = Scratch::create_with(&parent.join("store"), OP, &mut |_| {
        fs::write(old.join("swap"), b"theirs").unwrap();
        fs::rename(old.join("swap"), old.join("store.sqlite3-wal")).unwrap();
    });
    assert!(matches!(result, Err(Error::Replaced)));
    assert_eq!(fs::read(old.join("store.sqlite3-wal")).unwrap(), b"theirs");
    fs::remove_dir_all(parent).unwrap();
}

/// A foreign-owned directory cannot be created without root, so the decision function is
/// tested directly with an owner that differs from the directory's.
#[test]
fn a_directory_owned_by_another_user_is_refused() {
    use std::os::unix::fs::MetadataExt;
    let parent = parent("owner");
    let metadata = fs::symlink_metadata(&parent).unwrap();
    assert!(require_owner(&metadata, metadata.uid()).is_ok());
    assert!(matches!(
        require_owner(&metadata, metadata.uid().wrapping_add(1)),
        Err(Error::InvalidRoot)
    ));
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn a_snapshot_scratch_replaced_by_another_directory_is_not_removed() {
    let parent = parent("snapshot");
    let scratch = crate::recovery_copy::Scratch::create(&parent).unwrap();
    let moved = parent.join("moved");
    let path = fs::read_dir(&parent)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".hf-snapshot-")
        })
        .unwrap();
    fs::rename(&path, &moved).unwrap();
    fs::rename(parent.join("store"), &path).unwrap();
    fs::write(path.join("keep"), b"x").unwrap();
    assert!(matches!(scratch.cleanup(), Err(Error::Replaced)));
    drop(scratch);
    assert_eq!(fs::read(path.join("keep")).unwrap(), b"x");
    fs::remove_dir_all(parent).unwrap();
}
