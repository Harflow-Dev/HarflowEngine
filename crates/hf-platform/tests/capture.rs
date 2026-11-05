#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use hf_domain::{fixture::Stage, ownership::Id};
use hf_platform::fixture::{FIXED_STDERR, FIXED_STDOUT, RawCapture};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-b2c-capture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn root(&self) -> PathBuf {
        let path = self.0.join("store");
        fs::create_dir(&path).unwrap();
        path
    }
    fn stream(&self, operation: &Id, name: &str) -> PathBuf {
        self.0
            .join(format!("r03-b2c.{}.{name}", operation.as_str()))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn id() -> Id {
    Id::new("11111111111111111111111111111111".into()).unwrap()
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

#[test]
fn raw_capture_appends_exact_binary_streams_and_syncs_before_returning_offsets() {
    let scratch = Scratch::new();
    let root = scratch.root();
    let operation = id();
    let mut capture = RawCapture::create(&root, &operation, deadline()).unwrap();
    let identities = capture.identities();
    assert_ne!(identities.0, identities.1);
    let stdout_metadata = fs::metadata(scratch.stream(&operation, "stdout")).unwrap();
    assert_eq!(identities.0.device, stdout_metadata.dev());
    assert_eq!(identities.0.inode, stdout_metadata.ino());
    let mut stages = Vec::new();
    let offsets = capture
        .append(0, 0, FIXED_STDOUT, FIXED_STDERR, deadline(), |stage| {
            stages.push(stage);
            Ok(())
        })
        .unwrap();
    assert_eq!(stages, [Stage::CaptureBeforeSync, Stage::CaptureAfterSync]);
    assert_eq!(offsets.stdout, FIXED_STDOUT.len() as u64);
    assert_eq!(offsets.stderr, FIXED_STDERR.len() as u64);
    assert_eq!(read(&scratch.stream(&operation, "stdout")), FIXED_STDOUT);
    assert_eq!(read(&scratch.stream(&operation, "stderr")), FIXED_STDERR);
}

#[test]
fn failed_pre_sync_observation_never_returns_offsets_or_allows_replaying_the_extent() {
    let scratch = Scratch::new();
    let root = scratch.root();
    let operation = id();
    let mut capture = RawCapture::create(&root, &operation, deadline()).unwrap();
    let result = capture.append(0, 0, FIXED_STDOUT, FIXED_STDERR, deadline(), |stage| {
        if stage == Stage::CaptureBeforeSync {
            Err(std::io::Error::other("injected pre-sync stop"))
        } else {
            Ok(())
        }
    });
    assert!(result.is_err());
    assert_eq!(read(&scratch.stream(&operation, "stdout")), FIXED_STDOUT);
    assert_eq!(read(&scratch.stream(&operation, "stderr")), FIXED_STDERR);
    assert!(
        capture
            .append(
                FIXED_STDOUT.len() as u64,
                FIXED_STDERR.len() as u64,
                b"x",
                b"y",
                deadline(),
                |_| Ok(())
            )
            .is_err()
    );
}

#[test]
fn failed_post_sync_publication_observation_never_returns_offsets_or_allows_reuse() {
    let scratch = Scratch::new();
    let root = scratch.root();
    let operation = id();
    let mut capture = RawCapture::create(&root, &operation, deadline()).unwrap();
    let result = capture.append(0, 0, FIXED_STDOUT, FIXED_STDERR, deadline(), |stage| {
        if stage == Stage::CaptureAfterSync {
            Err(std::io::Error::other("injected post-sync stop"))
        } else {
            Ok(())
        }
    });
    assert!(result.is_err());
    assert_eq!(read(&scratch.stream(&operation, "stdout")), FIXED_STDOUT);
    assert_eq!(read(&scratch.stream(&operation, "stderr")), FIXED_STDERR);
    assert!(
        capture
            .append(
                FIXED_STDOUT.len() as u64,
                FIXED_STDERR.len() as u64,
                b"x",
                b"y",
                deadline(),
                |_| Ok(())
            )
            .is_err()
    );
}

#[test]
fn path_replacement_at_sync_gate_is_rechecked_before_offsets_return() {
    let scratch = Scratch::new();
    let root = scratch.root();
    let operation = id();
    let mut capture = RawCapture::create(&root, &operation, deadline()).unwrap();
    let stdout = scratch.stream(&operation, "stdout");
    let saved = scratch.0.join("saved-at-sync");
    let result = capture.append(0, 0, FIXED_STDOUT, FIXED_STDERR, deadline(), |stage| {
        if stage == Stage::CaptureAfterSync {
            fs::rename(&stdout, &saved)?;
            fs::write(&stdout, b"replacement")?;
        }
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(read(&saved), FIXED_STDOUT);
    assert_eq!(read(&stdout), b"replacement");
    fs::remove_file(&stdout).unwrap();
    fs::rename(&saved, &stdout).unwrap();
    assert!(
        capture
            .append(
                FIXED_STDOUT.len() as u64,
                FIXED_STDERR.len() as u64,
                b"x",
                b"y",
                deadline(),
                |_| Ok(())
            )
            .is_err()
    );
}

#[test]
fn raw_capture_refuses_path_replacement_and_hard_link_identity_changes() {
    use std::os::unix::fs::symlink;

    let scratch = Scratch::new();
    let root = scratch.root();
    let operation = id();
    let mut capture = RawCapture::create(&root, &operation, deadline()).unwrap();
    let stdout = scratch.stream(&operation, "stdout");
    let displaced = scratch.0.join("displaced");
    fs::rename(&stdout, &displaced).unwrap();
    fs::write(&stdout, b"replacement").unwrap();
    assert!(
        capture
            .append(0, 0, b"x", b"y", deadline(), |_| Ok(()))
            .is_err()
    );

    let other_operation = Id::new("22222222222222222222222222222222".into()).unwrap();
    let mut linked = RawCapture::create(&root, &other_operation, deadline()).unwrap();
    let linked_stdout = scratch.stream(&other_operation, "stdout");
    let alias = scratch.0.join("alias");
    fs::hard_link(&linked_stdout, &alias).unwrap();
    assert!(
        linked
            .append(0, 0, b"x", b"y", deadline(), |_| Ok(()))
            .is_err()
    );
    fs::remove_file(&alias).unwrap();

    let third_operation = Id::new("33333333333333333333333333333333".into()).unwrap();
    let mut symlinked = RawCapture::create(&root, &third_operation, deadline()).unwrap();
    let linked_path = scratch.stream(&third_operation, "stdout");
    let target = scratch.0.join("target");
    fs::rename(&linked_path, &target).unwrap();
    symlink(&target, &linked_path).unwrap();
    assert!(
        symlinked
            .append(0, 0, b"x", b"y", deadline(), |_| Ok(()))
            .is_err()
    );
}

#[test]
fn per_stream_total_cap_and_chunk_limit_are_enforced_before_append() {
    let scratch = Scratch::new();
    let root = scratch.root();
    let operation = id();
    let mut capture = RawCapture::create(&root, &operation, deadline()).unwrap();
    let chunk = vec![0xA5; 16_384];
    let mut stdout = 0;
    let mut stderr = 0;
    for _ in 0..4 {
        let offsets = capture
            .append(stdout, stderr, &chunk, &chunk, deadline(), |_| Ok(()))
            .unwrap();
        stdout = offsets.stdout;
        stderr = offsets.stderr;
    }
    assert_eq!((stdout, stderr), (65_536, 65_536));
    assert!(
        capture
            .append(stdout, stderr, b"x", b"x", deadline(), |_| Ok(()))
            .is_err()
    );
    assert_eq!(read(&scratch.stream(&operation, "stdout")).len(), 65_536);
    assert_eq!(read(&scratch.stream(&operation, "stderr")).len(), 65_536);
    let chunk_operation = Id::new("55555555555555555555555555555555".into()).unwrap();
    let mut chunk_capture = RawCapture::create(&root, &chunk_operation, deadline()).unwrap();
    assert!(
        chunk_capture
            .append(0, 0, &vec![0; 16_385], b"", deadline(), |_| Ok(()))
            .is_err()
    );
    assert!(read(&scratch.stream(&chunk_operation, "stdout")).is_empty());
    assert!(read(&scratch.stream(&chunk_operation, "stderr")).is_empty());
}
