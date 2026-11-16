#![cfg(target_os = "macos")]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! The bounded no-follow inventory walk over real directories.
use hf_application::ctl::state_inspect::InventoryPort;
use hf_domain::ctl::{
    recovery_proposal::{EntryBody, InventoryFailure},
    state_root::StateRoot,
};
use hf_platform::ctl::inventory::FsInventory;
use std::{
    fs,
    os::unix::fs::symlink,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = fs::canonicalize(std::env::temp_dir()).unwrap();
        let path = base.join(format!(
            "hf-inventory-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn root(&self) -> StateRoot {
        StateRoot::from_normalized(self.0.to_str().unwrap().to_string())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn walk(
    fixture: &Fixture,
    bound: usize,
) -> Result<hf_domain::ctl::recovery_proposal::Inventory, InventoryFailure> {
    FsInventory.walk(&fixture.root(), bound)
}

#[test]
fn symlinks_recorded_not_followed() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("data")).unwrap();
    fs::write(fixture.0.join("data/inner"), b"x").unwrap();
    symlink("data", fixture.0.join("link")).unwrap();
    symlink("/nonexistent/target", fixture.0.join("dangling")).unwrap();
    let inventory = walk(&fixture, 100).unwrap();
    let link = &inventory.entries["link"];
    assert_eq!(link.body, EntryBody::Link("data".to_string()));
    assert_eq!(
        inventory.entries["dangling"].body,
        EntryBody::Link("/nonexistent/target".to_string())
    );
    assert!(
        matches!(&inventory.entries["data"].body, EntryBody::Directory(c) if c.contains_key("inner"))
    );
}

#[test]
fn excluded_names_and_lock_identity() {
    let fixture = Fixture::new();
    for name in [
        ".harflow-store.lock",
        "store.lock",
        "io.lock",
        "store.recovery",
        "store.recovery.tmp",
        "store.sqlite3-shm",
        ".harflow-recovery.json",
        "kept",
    ] {
        fs::write(fixture.0.join(name), b"x").unwrap();
    }
    let inventory = walk(&fixture, 100).unwrap();
    assert_eq!(inventory.entries.keys().collect::<Vec<_>>(), ["kept"]);
    assert!(inventory.lock.is_some());
    assert!(!inventory.has_reference_state());
}

#[test]
fn reference_state_is_detected() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("tasks")).unwrap();
    assert!(!walk(&fixture, 100).unwrap().has_reference_state());
    fs::write(fixture.0.join("tasks/t1"), b"{}").unwrap();
    assert!(walk(&fixture, 100).unwrap().has_reference_state());
}

#[test]
fn special_entry_is_unsafe_root() {
    let fixture = Fixture::new();
    let status = std::process::Command::new("mkfifo")
        .arg(fixture.0.join("fifo"))
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(walk(&fixture, 100), Err(InventoryFailure::Special));
}

#[test]
fn entry_bound_counts_every_entry() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("d")).unwrap();
    for name in ["a", "b", "d/c"] {
        fs::write(fixture.0.join(name), b"x").unwrap();
    }
    assert!(walk(&fixture, 4).is_ok());
    assert_eq!(walk(&fixture, 3), Err(InventoryFailure::TooLarge));
}
