#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! A recovery that starts while an opener is between the lease-free gate read and its
//! lease: the record read under the lease (gate points 2 and 3) or the re-read after a
//! failed identification reports `RecoveryInterrupted`, never the marker or database
//! error the moves cause.
mod common;
mod read_session_support;
mod recovery_gate_support;
use common::lease::FileLeasePort;
use hf_application::{
    lease::{LeaseError, LeaseGuard, LeaseMode, LeasePort},
    recovery::RecoveryError,
};
use hf_storage::Error;
use read_session_support::seeded;
use recovery_gate_support::{
    OP, OPENERS, Outcome, copy_root, move_out, record_bytes, write_record,
};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// Writes the `pending` record when an opener first asks for `store.lock`: after its
/// lease-free gate read and identification, before its lease.
struct PendingAtLease {
    root: PathBuf,
    calls: AtomicUsize,
}

impl LeasePort for PendingAtLease {
    fn acquire(
        &self,
        path: &Path,
        mode: LeaseMode,
        create: bool,
        deadline: Instant,
    ) -> Result<Box<dyn LeaseGuard>, LeaseError> {
        if path.file_name().is_some_and(|name| name == "store.lock")
            && self.calls.fetch_add(1, Ordering::SeqCst) == 0
        {
            write_record(&self.root, "pending");
        }
        FileLeasePort.acquire(path, mode, create, deadline)
    }
}

#[test]
fn pending_landing_between_lease_free_read_and_lease_refuses_in_claim() {
    let seeded = seeded(3);
    for (index, (name, opener)) in OPENERS.iter().enumerate() {
        let parent = seeded.scratch.path().join(format!("opener-{index}"));
        std::fs::create_dir(&parent).unwrap();
        let root = copy_root(&seeded.root, &parent, "state");
        let port = Arc::new(PendingAtLease {
            root: root.clone(),
            calls: AtomicUsize::new(0),
        });
        let outcome = opener(&root, port.clone());
        assert!(
            port.calls.load(Ordering::SeqCst) >= 1,
            "{name}: the opener reached its lease"
        );
        assert!(
            matches!(
                outcome,
                Outcome::Store(Error::RecoveryInterrupted)
                    | Outcome::Recovery(RecoveryError::Unavailable)
            ),
            "{name}: {outcome:?}"
        );
    }
}

/// Takes `store.lock` exclusively as the maintenance session does, retrying while an
/// opener holds it shared.
fn exclusive(root: &Path) -> File {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("store.lock"))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while file.try_lock().is_err() {
        assert!(Instant::now() < deadline, "store.lock stayed held");
        std::thread::yield_now();
    }
    file
}

/// One recovery step in the real order: the exclusive lock, the `pending` record, then the
/// move. Never undone, so a moved file always has the record before it. The record is
/// published atomically (temporary file and rename) as the production writer does, but
/// without its flushes, which would make the opener win every race.
fn recover_concurrently(root: &Path, data: &Path, moved: &str, spin: u32) {
    for _ in 0..spin {
        std::hint::spin_loop();
    }
    let lock = exclusive(root);
    let temporary = root.join("store.recovery.race-tmp");
    std::fs::write(&temporary, record_bytes(OP, "pending")).unwrap();
    std::fs::rename(&temporary, root.join("store.recovery")).unwrap();
    move_out(root, data, moved);
    drop(lock);
}

/// No deterministic seam sits between gate point 1 and `Binding::capture`, so the race is
/// run many times with varied timing. Allowed answers: the opener finished first
/// (opened), lost the lease race (`Busy`), or was refused by the gate. Never `Io`,
/// `InvalidRoot` or `Incompatible` from the moved file.
#[test]
fn marker_or_database_moved_between_first_gate_and_capture_reports_recovery_interrupted() {
    let seeded = seeded(3);
    let mut interrupted = 0;
    for moved in ["store.identity", "store.sqlite3"] {
        for (index, (name, opener)) in OPENERS.iter().enumerate() {
            for round in 0..40u32 {
                let parent = seeded
                    .scratch
                    .path()
                    .join(format!("{moved}-{index}-{round}"));
                std::fs::create_dir(&parent).unwrap();
                let root = copy_root(&seeded.root, &parent, "state");
                let data = parent.join("data");
                let barrier = Barrier::new(2);
                let outcome = std::thread::scope(|scope| {
                    scope.spawn(|| {
                        barrier.wait();
                        recover_concurrently(&root, &data, moved, (round % 8) * 2_000);
                    });
                    barrier.wait();
                    opener(&root, common::lease::lease_port())
                });
                let allowed = matches!(
                    outcome,
                    Outcome::Opened
                        | Outcome::Store(
                            Error::RecoveryInterrupted | Error::Busy | Error::DeadlineExceeded
                        )
                        | Outcome::Recovery(
                            RecoveryError::Unavailable
                                | RecoveryError::Busy
                                | RecoveryError::DeadlineExceeded
                        )
                );
                assert!(allowed, "{moved}, {name}, round {round}: {outcome:?}");
                interrupted += usize::from(outcome.is_interrupted());
            }
        }
    }
    eprintln!("gate refusals during the race: {interrupted}");
}

/// A recovery replacing a `committed` record with `pending` by atomic rename while an
/// opener reads it: the gate's listed read retries a replacement, so the opener either
/// passes or refuses as `RecoveryInterrupted`, never `Incompatible`. No seam sits between
/// `lstat` and the open, so this is a bounded repeated race (the in-crate
/// `read_listed_reports_a_racing_replacement_or_removal_without_an_error` shows the window
/// is reachable).
#[test]
fn a_committed_record_replaced_by_pending_during_the_gate_read_is_never_incompatible() {
    let seeded = seeded(3);
    for (index, (name, opener)) in OPENERS.iter().enumerate() {
        for round in 0..40u32 {
            let parent = seeded.scratch.path().join(format!("swap-{index}-{round}"));
            std::fs::create_dir(&parent).unwrap();
            let root = copy_root(&seeded.root, &parent, "state");
            std::fs::write(root.join("store.recovery"), record_bytes(OP, "committed")).unwrap();
            let barrier = Barrier::new(2);
            let outcome = std::thread::scope(|scope| {
                scope.spawn(|| {
                    barrier.wait();
                    for _ in 0..(round % 8) * 2_000 {
                        std::hint::spin_loop();
                    }
                    let temporary = root.join("store.recovery.race-tmp");
                    std::fs::write(&temporary, record_bytes(OP, "pending")).unwrap();
                    std::fs::rename(&temporary, root.join("store.recovery")).unwrap();
                });
                barrier.wait();
                opener(&root, common::lease::lease_port())
            });
            let allowed = matches!(
                outcome,
                Outcome::Opened
                    | Outcome::Store(Error::RecoveryInterrupted)
                    | Outcome::Recovery(RecoveryError::Unavailable)
            );
            assert!(allowed, "{name}, round {round}: {outcome:?}");
        }
    }
}
