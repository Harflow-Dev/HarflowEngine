#![cfg(unix)]
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
pub mod lease;

pub struct Fixture(PathBuf);

impl Fixture {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hf-storage-test-{}-{}",
            std::process::id(),
            fresh_suffix()
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fresh_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}",
        Instant::now().elapsed().as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

pub fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}

pub fn default_limits() -> hf_storage::Limits {
    hf_storage::Limits::default()
}

pub fn snapshot_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries = Vec::new();
    snapshot_into(root, &mut entries);
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn snapshot_into(root: &Path, entries: &mut Vec<(PathBuf, Vec<u8>)>) {
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        let metadata = fs::symlink_metadata(&path).unwrap();
        let bytes = if metadata.file_type().is_dir() {
            Vec::new()
        } else {
            fs::read(&path).unwrap()
        };
        entries.push((path.clone(), bytes));
        if metadata.file_type().is_dir() {
            snapshot_into(&path, entries);
        }
    }
}
