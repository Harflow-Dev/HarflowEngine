#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
