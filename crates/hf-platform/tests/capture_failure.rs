#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use hf_domain::{fixture::Stage, ownership::Id};
use hf_platform::fixture::{FIXED_STDERR, FIXED_STDOUT, RawCapture};
use std::{
    fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-b2c-deadline-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn post_sync_deadline_expiry_returns_no_offsets_and_permanently_stops_the_capture() {
    let scratch = Scratch::new();
    let path = &scratch.0;
    let root = path.join("store");
    fs::create_dir(&root).unwrap();
    let operation = Id::new("66666666666666666666666666666666".into()).unwrap();
    let mut capture =
        RawCapture::create(&root, &operation, Instant::now() + Duration::from_secs(1)).unwrap();
    let until = Instant::now() + Duration::from_millis(250);
    let result = capture.append(0, 0, FIXED_STDOUT, FIXED_STDERR, until, |stage| {
        if stage == Stage::CaptureAfterSync {
            while Instant::now() < until {
                thread::yield_now();
            }
        }
        Ok(())
    });
    assert!(
        result.is_err(),
        "expired post-sync observation returned offsets"
    );
    assert_eq!(
        fs::read(path.join(format!("r03-b2c.{}.stdout", operation.as_str()))).unwrap(),
        FIXED_STDOUT
    );
    assert_eq!(
        fs::read(path.join(format!("r03-b2c.{}.stderr", operation.as_str()))).unwrap(),
        FIXED_STDERR
    );
    assert!(
        capture
            .append(
                FIXED_STDOUT.len() as u64,
                FIXED_STDERR.len() as u64,
                b"again",
                b"again",
                Instant::now() + Duration::from_secs(1),
                |_| Ok(())
            )
            .is_err()
    );
}
