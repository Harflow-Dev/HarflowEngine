#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[allow(dead_code, unused_imports)]
#[path = "process_primitives/common.rs"]
mod process_common;
#[path = "recovery/support.rs"]
mod support;
use hf_application::recovery::{RecoveryIdentityPort, RecoveryRawPort};
use hf_domain::{
    capture::{CaptureBinding, CaptureProgress, FileIdentity},
    ownership::{Fence, Id, OwnerClaim},
    process::ProcessIdentity,
    recovery::{ProcessState, StreamObservation},
};
use hf_platform::recovery::{MacRecoveryIdentity, RawCaptureObserver};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    time::{Duration, Instant},
};

const BOOT: &str = "abcdef01-2345-6789-abcd-ef0123456789";
const OP: &str = "11111111111111111111111111111111";

#[test]
fn actual_current_process_is_observed_as_present_original() {
    let pid = std::process::id();
    let (seconds, micros, status) = process_common::independent_os_probe(pid);
    assert!(
        matches!(status, 1..=4),
        "test process must be live: {status}"
    );
    let expected = ProcessIdentity::new(
        pid,
        seconds,
        micros,
        process_common::independent_boot_uuid(),
    )
    .expect("independent process identity");
    let evidence = MacRecoveryIdentity.observe(&expected, deadline()).unwrap();
    assert_eq!(evidence.expected, expected);
    assert_eq!(evidence.actual.as_ref(), Some(&expected));
    assert_eq!(evidence.state, ProcessState::Present);
    assert_eq!(evidence.boot.as_deref(), Some(expected.boot_uuid()));
}

#[test]
fn raw_observer_reads_fixed_files_without_a_writer_lock_and_checks_file_identity_and_extent() {
    let fixture = support::Fixture::new();
    let stdout = fixture.raw_path("stdout");
    let stderr = fixture.raw_path("stderr");
    fs::write(&stdout, b"out-data").unwrap();
    fs::write(&stderr, b"err").unwrap();
    let binding = binding(&stdout, &stderr);
    let observer = RawCaptureObserver::new(fixture.root());
    let progress = CaptureProgress {
        revision: 1,
        stdout: 4,
        stderr: 3,
        stdout_eof: false,
        stderr_eof: false,
        loss: false,
    };
    let evidence = observer.observe(&binding, &progress, deadline()).unwrap();
    assert!(matches!(
        evidence.stdout,
        StreamObservation::Present { length: 8, .. }
    ));
    assert!(matches!(
        evidence.stderr,
        StreamObservation::Present { length: 3, .. }
    ));

    fs::write(&stdout, b"out").unwrap();
    let short = observer.observe(&binding, &progress, deadline()).unwrap();
    assert_eq!(
        short.stdout,
        StreamObservation::Changed,
        "truncation below indexed offset is not healthy evidence"
    );

    fs::remove_file(&stdout).unwrap();
    let missing = observer.observe(&binding, &progress, deadline()).unwrap();
    assert_eq!(missing.stdout, StreamObservation::Missing);
}

#[test]
fn replacement_and_oversized_raw_files_are_unqualified_observations() {
    let fixture = support::Fixture::new();
    let stdout = fixture.raw_path("stdout");
    let stderr = fixture.raw_path("stderr");
    fs::write(&stdout, b"out").unwrap();
    fs::write(&stderr, b"err").unwrap();
    let binding = binding(&stdout, &stderr);
    let saved = fixture.raw_path("saved.stdout");
    fs::rename(&stdout, &saved).unwrap();
    fs::write(&stdout, b"out").unwrap();
    fs::write(&stderr, vec![b'x'; 65_537]).unwrap();
    let evidence = RawCaptureObserver::new(fixture.root())
        .observe(
            &binding,
            &CaptureProgress {
                revision: 0,
                stdout: 0,
                stderr: 0,
                stdout_eof: false,
                stderr_eof: false,
                loss: false,
            },
            deadline(),
        )
        .unwrap();
    assert_eq!(
        evidence.stdout,
        StreamObservation::Changed,
        "replacement inode is not rebound"
    );
    assert_eq!(
        evidence.stderr,
        StreamObservation::Changed,
        "the fixed per-stream bound is enforced"
    );
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
fn id(byte: u8) -> Id {
    Id::new(format!("{byte:02x}").repeat(16)).unwrap()
}
fn identity(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 42, 7, BOOT.into()).unwrap()
}
fn binding(stdout: &std::path::Path, stderr: &std::path::Path) -> CaptureBinding {
    let owner = OwnerClaim {
        owner: id(6),
        nonce: id(7),
        process: identity(111),
    };
    CaptureBinding {
        fence: Fence {
            operation: Id::new(OP.into()).unwrap(),
            generation: id(2),
            attempt: id(5),
            owner,
            revision: 3,
        },
        child: identity(112),
        stdout: file_identity(stdout),
        stderr: file_identity(stderr),
    }
}
fn file_identity(path: &std::path::Path) -> FileIdentity {
    let metadata = fs::symlink_metadata(path).unwrap();
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}
