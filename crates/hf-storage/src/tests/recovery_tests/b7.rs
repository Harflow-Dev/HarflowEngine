#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::*;
use hf_application::lease::{LeaseError, LeaseGuard, LeaseMode, LeasePort};
use std::{
    path::Path,
    sync::{Arc, Mutex, mpsc},
    time::Instant,
};

fn session(root: &Path, scratch: &Path) -> RecoveryReadSession {
    RecoveryReadSession::new(
        root,
        scratch,
        Limits::default(),
        crate::tests::lease::lease_port(),
    )
}

fn assert_unavailable(root: &Path, scratch: &Path, operation: &Id) {
    let result = session(root, scratch).read(operation, deadline());
    assert_eq!(
        result,
        Err(hf_application::recovery::RecoveryError::Unavailable)
    );
}

fn assert_invalid(root: &Path, scratch: &Path, operation: &Id) {
    let result = session(root, scratch).read(operation, deadline());
    assert_eq!(
        result,
        Err(hf_application::recovery::RecoveryError::Invalid)
    );
}

fn setup(label: &str) -> (Fixture, Id, std::path::PathBuf) {
    let fixture = Fixture::new(label);
    let root = fixture.root();
    let store = fixture.store();
    let operation = seed_terminal(&store, 101);
    drop(store);
    let scratch = root.parent().unwrap().join("scratch");
    fs::create_dir(&scratch).unwrap();
    (fixture, operation, scratch)
}

fn marker_generation(root: &Path) -> String {
    crate::wal::read_marker_at(root, true).unwrap().generation
}

#[test]
fn failed_and_commit_lost_fences_refuse_as_unavailable_before_scratch_access() {
    for state in ["failed", "commit_lost"] {
        let (fixture, operation, _) = setup(&format!("b7-fence-{state}"));
        let root = fixture.root();
        let generation = marker_generation(&root);
        fs::write(
            root.join("store.fence"),
            format!("harflow-fence-1\n{generation}\n{state}\n"),
        )
        .unwrap();
        let bad_scratch = root.parent().unwrap().join("not-a-directory");
        fs::write(&bad_scratch, b"occupied").unwrap();
        assert_unavailable(&root, &bad_scratch, &operation);
        assert_eq!(fs::read(&bad_scratch).unwrap(), b"occupied");
    }
}

#[test]
fn pending_recovery_gate_refuses_as_unavailable_before_scratch_access() {
    let (fixture, operation, _) = setup("b7-pending");
    let root = fixture.root();
    fs::write(
        root.join("store.recovery"),
        format!("harflow-recovery-1\n{}\npending\n", operation.as_str()),
    )
    .unwrap();
    let bad_scratch = root.parent().unwrap().join("not-a-directory");
    fs::write(&bad_scratch, b"occupied").unwrap();
    assert_unavailable(&root, &bad_scratch, &operation);
}

#[test]
fn missing_wal_and_pending_acceptance_refuse_as_unavailable_without_healing() {
    let (fixture, operation, scratch) = setup("b7-missing-wal");
    let root = fixture.root();
    let before = source_hashes(&root);
    fs::remove_file(root.join("store.sqlite3-wal")).unwrap();
    let after_removal = source_hashes(&root);
    assert_unavailable(&root, &scratch, &operation);
    assert_eq!(after_removal, source_hashes(&root));
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
    assert_ne!(before, after_removal);
    drop(fixture);

    let (fixture, operation, scratch) = setup("b7-pending-acceptance");
    let root = fixture.root();
    let generation = marker_generation(&root);
    fs::write(
        root.join("store.wal-acceptance"),
        format!("harflow-wal-acceptance-1\n{generation}\npending\n"),
    )
    .unwrap();
    let before = source_hashes(&root);
    assert_unavailable(&root, &scratch, &operation);
    assert_eq!(before, source_hashes(&root));
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
}

#[test]
fn watermark_ahead_without_fence_refuses_as_unavailable_and_does_not_heal() {
    let (fixture, operation, scratch) = setup("b7-watermark-ahead");
    let root = fixture.root();
    let generation = marker_generation(&root);
    let mut bytes = vec![0; hf_application::watermark::FILE_LEN];
    let payload = hf_application::watermark::encode_payload(&generation, 99).unwrap();
    bytes[..payload.len()].copy_from_slice(&payload);
    fs::write(root.join("store.watermark"), bytes).unwrap();
    let before = source_hashes(&root);
    assert_unavailable(&root, &scratch, &operation);
    assert_eq!(before, source_hashes(&root));
    assert!(!root.join("store.fence").exists());
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
}

#[test]
fn missing_watermark_refuses_as_unavailable_without_copy_residue() {
    let (fixture, operation, scratch) = setup("b7-watermark-missing");
    let root = fixture.root();
    fs::remove_file(root.join("store.watermark")).unwrap();
    let before = source_hashes(&root);
    assert_unavailable(&root, &scratch, &operation);
    assert_eq!(before, source_hashes(&root));
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
}

#[test]
fn malformed_watermark_refuses_as_invalid_without_copy_residue() {
    let (fixture, operation, scratch) = setup("b7-watermark-malformed");
    let root = fixture.root();
    fs::write(root.join("store.watermark"), b"bad watermark").unwrap();
    let before = source_hashes(&root);
    assert_invalid(&root, &scratch, &operation);
    assert_eq!(before, source_hashes(&root));
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn nonregular_and_symlink_wal_are_refused_without_following_or_scratch_residue() {
    use std::os::unix::fs::symlink;

    for link in [false, true] {
        let (fixture, operation, scratch) = setup("b7-wal-type");
        let root = fixture.root();
        let wal = root.join("store.sqlite3-wal");
        fs::remove_file(&wal).unwrap();
        if link {
            let target = root.parent().unwrap().join("wal-target");
            fs::write(&target, b"sentinel").unwrap();
            symlink(&target, &wal).unwrap();
            // Binding::io validates sidecar types before WAL precheck and maps this refusal
            // as InvalidRoot, including for a symlink whose target is otherwise readable.
            assert_invalid(&root, &scratch, &operation);
            assert_eq!(fs::read(target).unwrap(), b"sentinel");
        } else {
            // A directory is a stable non-regular WAL that cannot block in open().
            fs::create_dir(&wal).unwrap();
            // Binding::io's sidecar validation precedes precheck's WalMissing result.
            assert_invalid(&root, &scratch, &operation);
        }
        assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
    }
}

struct PauseIoLease {
    reached: mpsc::SyncSender<()>,
    resume: Mutex<mpsc::Receiver<()>>,
}

impl LeasePort for PauseIoLease {
    fn acquire(
        &self,
        path: &Path,
        mode: LeaseMode,
        create: bool,
        deadline: Instant,
    ) -> Result<Box<dyn LeaseGuard>, LeaseError> {
        let guard = crate::tests::lease::FileLeasePort.acquire(path, mode, create, deadline)?;
        if path.file_name().is_some_and(|name| name == "io.lock") {
            self.reached.send(()).map_err(|_| LeaseError::Unavailable)?;
            self.resume
                .lock()
                .unwrap()
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| LeaseError::DeadlineExceeded)?;
        }
        Ok(guard)
    }
}

#[test]
fn copy_under_io_lock_matches_watermark() {
    let (fixture, operation, scratch) = setup("b7-copy-io-lock");
    let root = fixture.root();
    let (reached_send, reached_receive) = mpsc::sync_channel(1);
    let (resume_send, resume_receive) = mpsc::sync_channel(1);
    let leases: Arc<dyn LeasePort> = Arc::new(PauseIoLease {
        reached: reached_send,
        resume: Mutex::new(resume_receive),
    });
    let read_root = root.clone();
    let read_scratch = scratch.clone();
    let read_operation = operation.clone();
    let reader = std::thread::spawn(move || {
        RecoveryReadSession::new(read_root, read_scratch, Limits::default(), leases)
            .read(&read_operation, deadline())
    });

    reached_receive
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let contender = crate::tests::lease::FileLeasePort.acquire(
        &root.join("io.lock"),
        LeaseMode::Exclusive,
        false,
        deadline(),
    );
    assert!(matches!(contender, Err(LeaseError::Busy)));
    resume_send.send(()).unwrap();
    let result = reader.join().unwrap();
    assert!(result.unwrap().is_some());
    assert_eq!(fs::read_dir(scratch).unwrap().count(), 0);
}
