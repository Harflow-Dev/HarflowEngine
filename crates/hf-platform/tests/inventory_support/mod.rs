#![allow(dead_code)]
//! A scratch directory for the inventory walk tests.
use hf_application::ctl::state_inspect::InventoryPort;
use hf_domain::ctl::{
    recovery_proposal::{Inventory, InventoryFailure},
    state_root::StateRoot,
};
use hf_platform::ctl::inventory::FsInventory;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

pub struct Fixture(pub PathBuf);
impl Fixture {
    pub fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = fs::canonicalize(std::env::temp_dir()).unwrap();
        let path = base.join(format!(
            "hf-inventory-walk-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    pub fn root(&self) -> StateRoot {
        StateRoot::from_normalized(self.0.to_str().unwrap().to_string())
    }
    pub fn walk(&self, bound: usize) -> Result<Inventory, InventoryFailure> {
        FsInventory.walk(&self.root(), bound)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Directories left unreadable by a test must be removable.
        fn open_up(path: &std::path::Path) {
            let Ok(meta) = fs::symlink_metadata(path) else {
                return;
            };
            if meta.is_dir() {
                let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
                if let Ok(listing) = fs::read_dir(path) {
                    for child in listing.flatten() {
                        open_up(&child.path());
                    }
                }
            }
        }
        open_up(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}
