#![cfg(target_os = "macos")]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! The inventory walk against hostile directories: recorded attributes, exclusions,
//! non-UTF-8 link targets, unreadable directories and depth.
use hf_application::ctl::state_inspect::InventoryPort;
use hf_domain::ctl::{
    recovery_proposal::{EntryBody, InventoryFailure},
    state_root::StateRoot,
};
use hf_platform::ctl::inventory::FsInventory;
mod inventory_support;

use inventory_support::Fixture;
use std::{
    ffi::OsStr,
    fs,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, PermissionsExt, symlink},
        net::UnixListener,
    },
    path::PathBuf,
};

#[test]
fn recorded_attributes_are_the_no_follow_attributes_of_each_entry() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("file"), b"hello").unwrap();
    fs::create_dir(fixture.0.join("dir")).unwrap();
    symlink("file", fixture.0.join("link")).unwrap();
    let inventory = fixture.walk(100).unwrap();
    for (name, body_is) in [("file", "file"), ("dir", "dir"), ("link", "link")] {
        let meta = fs::symlink_metadata(fixture.0.join(name)).unwrap();
        let entry = &inventory.entries[name];
        assert_eq!(entry.stat.dev, i64::from(meta.dev() as i32), "{name}");
        assert_eq!(entry.stat.ino, meta.ino(), "{name}");
        assert_eq!(entry.stat.mode, meta.mode(), "{name}");
        assert_eq!(
            entry.stat.size,
            i64::try_from(meta.size()).unwrap(),
            "{name}"
        );
        assert_eq!(
            entry.stat.mtime_ns,
            meta.mtime() * 1_000_000_000 + meta.mtime_nsec(),
            "{name}"
        );
        let matches = match (&entry.body, body_is) {
            (EntryBody::File, "file") => true,
            (EntryBody::Directory(children), "dir") => children.is_empty(),
            (EntryBody::Link(target), "link") => target == "file",
            _ => false,
        };
        assert!(matches, "{name}: {:?}", entry.body);
    }
    let root = fs::symlink_metadata(&fixture.0).unwrap();
    assert_eq!(inventory.root.ino, root.ino());
    assert_eq!(inventory.root.dev, i64::from(root.dev() as i32));
}

#[test]
fn a_walk_is_repeatable_and_a_touched_entry_changes_it() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("a"), b"x").unwrap();
    fs::create_dir(fixture.0.join("d")).unwrap();
    fs::write(fixture.0.join("d/b"), b"y").unwrap();
    let first = fixture.walk(100).unwrap();
    assert_eq!(
        fixture.walk(100).unwrap(),
        first,
        "an unchanged tree walks the same"
    );
    let file = fs::OpenOptions::new()
        .write(true)
        .open(fixture.0.join("d/b"))
        .unwrap();
    file.set_modified(
        std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000),
    )
    .unwrap();
    assert_ne!(
        fixture.walk(100).unwrap(),
        first,
        "a changed mtime changes the inventory"
    );
}

#[test]
fn an_empty_root_is_an_empty_inventory_even_with_a_zero_bound() {
    let fixture = Fixture::new();
    let inventory = fixture.walk(0).unwrap();
    assert!(inventory.entries.is_empty());
    assert_eq!(inventory.lock, None);
    fs::write(fixture.0.join("one"), b"").unwrap();
    assert_eq!(fixture.walk(0), Err(InventoryFailure::TooLarge));
    assert!(fixture.walk(1).is_ok());
}

#[test]
fn excluded_top_level_names_do_not_count_toward_the_bound() {
    let fixture = Fixture::new();
    for name in [
        ".harflow-store.lock",
        "store.lock",
        "io.lock",
        "store.recovery",
        "store.recovery.tmp",
        "store.sqlite3-shm",
        ".harflow-recovery.json",
        ".harflow-recovery.json.pending",
    ] {
        fs::write(fixture.0.join(name), b"x").unwrap();
    }
    fs::write(fixture.0.join("kept"), b"x").unwrap();
    let inventory = fixture.walk(1).unwrap();
    assert_eq!(inventory.entries.keys().collect::<Vec<_>>(), ["kept"]);
}

#[test]
fn a_non_regular_excluded_top_level_name_is_unsafe() {
    use std::process::Command;
    for name in hf_domain::ctl::recovery_proposal::INVENTORY_EXCLUDED {
        for kind in ["fifo", "directory", "symlink"] {
            let fixture = Fixture::new();
            let path = fixture.0.join(name);
            match kind {
                "fifo" => assert!(
                    Command::new("mkfifo")
                        .arg(&path)
                        .status()
                        .unwrap()
                        .success()
                ),
                "directory" => fs::create_dir(&path).unwrap(),
                _ => symlink("target", &path).unwrap(),
            }
            let result = fixture.walk(100);
            if name == ".harflow-store.lock" {
                assert!(result.is_err(), "{name} {kind}");
            } else {
                assert_eq!(result, Err(InventoryFailure::Special), "{name} {kind}");
            }
        }
    }
}

#[test]
fn a_regular_excluded_name_is_skipped_and_not_inventoried() {
    let fixture = Fixture::new();
    for name in hf_domain::ctl::recovery_proposal::INVENTORY_EXCLUDED {
        fs::write(fixture.0.join(name), b"x").unwrap();
    }
    assert!(fixture.walk(0).unwrap().entries.is_empty());
}

#[test]
fn exclusions_apply_to_the_top_level_only() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("sub")).unwrap();
    for name in [
        "store.lock",
        "io.lock",
        "store.recovery",
        ".harflow-store.lock",
    ] {
        fs::write(fixture.0.join("sub").join(name), b"x").unwrap();
    }
    let inventory = fixture.walk(100).unwrap();
    let EntryBody::Directory(children) = &inventory.entries["sub"].body else {
        panic!("sub is a directory");
    };
    assert_eq!(children.len(), 4);
}

#[test]
fn the_lock_identity_is_the_stable_lock_files_identity() {
    let fixture = Fixture::new();
    assert_eq!(fixture.walk(10).unwrap().lock, None);
    let lock = fixture.0.join(".harflow-store.lock");
    fs::write(&lock, b"").unwrap();
    let meta = fs::symlink_metadata(&lock).unwrap();
    let identity = fixture.walk(10).unwrap().lock.unwrap();
    assert_eq!(
        (identity.dev, identity.ino),
        (i64::from(meta.dev() as i32), meta.ino())
    );
}

#[test]
fn a_lock_that_is_not_a_regular_file_cannot_be_walked() {
    let directory = Fixture::new();
    fs::create_dir(directory.0.join(".harflow-store.lock")).unwrap();
    assert_eq!(directory.walk(10), Err(InventoryFailure::Unreadable));
    let link = Fixture::new();
    fs::write(link.0.join("target"), b"").unwrap();
    symlink("target", link.0.join(".harflow-store.lock")).unwrap();
    assert_eq!(link.walk(10), Err(InventoryFailure::Unreadable));
}

#[test]
fn special_entries_nested_anywhere_are_unsafe() {
    let fifo = Fixture::new();
    fs::create_dir_all(fifo.0.join("a/b")).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(fifo.0.join("a/b/pipe"))
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(fifo.walk(100), Err(InventoryFailure::Special));
    let socket = Fixture::new();
    let _listener = UnixListener::bind(socket.0.join("sock")).unwrap();
    assert_eq!(socket.walk(100), Err(InventoryFailure::Special));
}

#[test]
fn a_link_to_a_special_file_or_a_loop_is_only_recorded() {
    let fixture = Fixture::new();
    let outside = Fixture::new();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(outside.0.join("pipe"))
            .status()
            .unwrap()
            .success()
    );
    symlink(outside.0.join("pipe"), fixture.0.join("to-fifo")).unwrap();
    symlink("self", fixture.0.join("self")).unwrap();
    symlink(&fixture.0, fixture.0.join("to-root")).unwrap();
    symlink("../..", fixture.0.join("up")).unwrap();
    let inventory = fixture.walk(100).unwrap();
    assert_eq!(inventory.entries.len(), 4);
    assert!(
        inventory
            .entries
            .values()
            .all(|e| matches!(e.body, EntryBody::Link(_)))
    );
    assert_eq!(
        inventory.entries["to-fifo"].body,
        EntryBody::Link(outside.0.join("pipe").to_str().unwrap().to_string())
    );
}

// APFS refuses file names that are not UTF-8, so only a link target can be non-UTF-8 here.
fn fixture_name(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsStr::from_bytes(bytes))
}

#[test]
fn a_link_target_that_is_not_utf8_cannot_be_inventoried() {
    let fixture = Fixture::new();
    symlink(fixture_name(&[b't', 0xfe]), fixture.0.join("link")).unwrap();
    assert_eq!(fixture.walk(10), Err(InventoryFailure::Unreadable));
}

#[test]
fn unusual_but_valid_names_are_recorded_verbatim() {
    let fixture = Fixture::new();
    let names = [
        ".hidden",
        "new\nline",
        "caf\u{e9}",
        "\u{1f600}",
        "sp ace",
        "quote\"",
        "back\\slash",
        "-dash",
    ];
    for name in names {
        fs::write(fixture.0.join(name), b"x").unwrap();
    }
    let inventory = fixture.walk(100).unwrap();
    let mut recorded: Vec<_> = inventory.entries.keys().cloned().collect();
    let mut expected: Vec<_> = names.iter().map(|n| (*n).to_string()).collect();
    recorded.sort();
    expected.sort();
    assert_eq!(recorded, expected);
}

#[test]
fn an_unreadable_directory_cannot_be_inventoried() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("closed")).unwrap();
    fs::write(fixture.0.join("closed/inner"), b"x").unwrap();
    fs::set_permissions(fixture.0.join("closed"), fs::Permissions::from_mode(0o000)).unwrap();
    // Permission bits do not bind the superuser.
    if fs::read_dir(fixture.0.join("closed")).is_ok() {
        return;
    }
    assert_eq!(fixture.walk(100), Err(InventoryFailure::Unreadable));
}

#[test]
fn nesting_deeper_than_the_depth_bound_is_too_large() {
    let nest = |levels: usize| {
        let fixture = Fixture::new();
        let mut path = fixture.0.clone();
        for _ in 0..levels {
            path.push("d");
        }
        fs::create_dir_all(&path).unwrap();
        fixture
    };
    assert!(nest(100).walk(1000).is_ok());
    assert!(nest(128).walk(1000).is_ok());
    assert_eq!(nest(129).walk(1000), Err(InventoryFailure::TooLarge));
}

#[test]
fn a_root_that_is_not_a_plain_directory_cannot_be_walked() {
    let fixture = Fixture::new();
    let target = fixture.0.join("real");
    fs::create_dir(&target).unwrap();
    symlink(&target, fixture.0.join("link")).unwrap();
    let via_link = StateRoot::from_normalized(fixture.0.join("link").to_str().unwrap().to_string());
    assert!(
        FsInventory.walk(&via_link, 100).is_err(),
        "a symlinked root is not followed"
    );
    fs::write(fixture.0.join("file"), b"").unwrap();
    let file = StateRoot::from_normalized(fixture.0.join("file").to_str().unwrap().to_string());
    assert!(FsInventory.walk(&file, 100).is_err());
    let missing =
        StateRoot::from_normalized(fixture.0.join("missing").to_str().unwrap().to_string());
    assert!(FsInventory.walk(&missing, 100).is_err());
}

#[test]
fn walking_changes_nothing() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("a"), b"x").unwrap();
    fs::create_dir(fixture.0.join("d")).unwrap();
    let before = fs::read_dir(&fixture.0).unwrap().count();
    let meta = fs::symlink_metadata(&fixture.0).unwrap();
    fixture.walk(100).unwrap();
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), before);
    let after = fs::symlink_metadata(&fixture.0).unwrap();
    assert_eq!(
        (after.mtime(), after.mtime_nsec()),
        (meta.mtime(), meta.mtime_nsec())
    );
}
