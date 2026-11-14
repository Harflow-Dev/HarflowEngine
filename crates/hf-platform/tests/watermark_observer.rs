#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The lease-free observer's read of `store.watermark`: bounded at 8192 bytes, a longer
//! file is `Oversize`, and a whole-length file without a valid block is read again after
//! short pauses before its bytes are reported.
use hf_application::{
    ctl::state_inspect::StateObserverPort,
    watermark::{self, FILE_LEN},
};
use hf_domain::ctl::{inspect::FileState, state_root::StateRoot};
use hf_platform::ctl::state_observer::FsStateObserver;
use std::{
    fs,
    os::unix::fs::symlink,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const GENERATION: &str = "0123456789abcdef0123456789abcdef";

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hf-platform-watermark-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let path = fs::canonicalize(path).unwrap();
        fs::create_dir(path.join("state")).unwrap();
        Self(path)
    }
    fn root(&self) -> PathBuf {
        self.0.join("state")
    }
    fn observe(&self) -> FileState {
        let root = StateRoot::from_normalized(self.root().to_string_lossy().into_owned());
        FsStateObserver.observe(&root).watermark
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn valid_file() -> Vec<u8> {
    watermark::initial_file(GENERATION).unwrap()
}

#[test]
fn a_whole_file_is_read_exactly_and_a_longer_one_is_oversize() {
    let scratch = Scratch::new();
    let path = scratch.root().join(watermark::NAME);
    assert!(matches!(scratch.observe(), FileState::Absent));
    fs::write(&path, valid_file()).unwrap();
    assert!(matches!(scratch.observe(), FileState::Bytes(bytes) if bytes == valid_file()));
    let short = valid_file()[..FILE_LEN - 1].to_vec();
    fs::write(&path, &short).unwrap();
    assert!(matches!(scratch.observe(), FileState::Bytes(bytes) if bytes == short));
    fs::write(&path, [valid_file().as_slice(), &[0]].concat()).unwrap();
    assert!(matches!(scratch.observe(), FileState::Oversize));
}

#[test]
fn a_non_regular_watermark_is_not_regular() {
    let scratch = Scratch::new();
    let path = scratch.root().join(watermark::NAME);
    let elsewhere = scratch.0.join("elsewhere");
    fs::write(&elsewhere, valid_file()).unwrap();
    symlink(&elsewhere, &path).unwrap();
    assert!(matches!(scratch.observe(), FileState::NotRegular));
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(matches!(scratch.observe(), FileState::NotRegular));
}

#[test]
fn a_whole_file_without_a_valid_block_is_retried_before_it_is_reported() {
    let scratch = Scratch::new();
    let path = scratch.root().join(watermark::NAME);
    let blank = vec![0u8; FILE_LEN];
    fs::write(&path, &blank).unwrap();
    let started = Instant::now();
    assert!(matches!(scratch.observe(), FileState::Bytes(bytes) if bytes == blank));
    assert!(
        started.elapsed() >= Duration::from_millis(3),
        "three retries 1 ms apart: {:?}",
        started.elapsed()
    );
    // A short or valid file is reported without retrying.
    fs::write(&path, &blank[..100]).unwrap();
    let started = Instant::now();
    assert!(matches!(scratch.observe(), FileState::Bytes(bytes) if bytes.len() == 100));
    fs::write(&path, valid_file()).unwrap();
    assert!(matches!(scratch.observe(), FileState::Bytes(bytes) if bytes == valid_file()));
    assert!(started.elapsed() < Duration::from_secs(1));
}
