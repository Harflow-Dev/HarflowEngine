#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use hf_application::recovery::{
    RecoveryError, RecoveryIdentityPort, RecoveryInspector, RecoveryRawPort, RecoveryStorePort,
};
use hf_domain::{
    capture::{CaptureBinding, CaptureProgress, CaptureSnapshot, FileIdentity},
    ownership::{Digest, Fence, Id, OwnerClaim, Phase, PreparedRequest, Snapshot},
    process::{ProcessExit, ProcessIdentity},
    recovery::{
        CaptureAssessment, ProcessEvidence, ProcessState, RawEvidence, RecoveryRecord,
        ReservationDisposition, SourceStamp, StreamObservation,
    },
};
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

const BOOT: &str = "abcdef01-2345-6789-abcd-ef0123456789";

struct Store(RecoveryRecord, Rc<Cell<u8>>);
impl RecoveryStorePort for Store {
    fn read(&self, _: &Id, _: Instant) -> Result<Option<RecoveryRecord>, RecoveryError> {
        self.1.set(self.1.get() + 1);
        Ok(Some(self.0.clone()))
    }
}
struct Identity;
impl RecoveryIdentityPort for Identity {
    fn observe(
        &self,
        expected: &ProcessIdentity,
        _: Instant,
    ) -> Result<ProcessEvidence, RecoveryError> {
        Ok(ProcessEvidence {
            expected: expected.clone(),
            state: ProcessState::AbsentPid,
            actual: None,
            boot: Some(BOOT.into()),
        })
    }
}
struct Raw {
    tail: bool,
}
impl RecoveryRawPort for Raw {
    fn observe(
        &self,
        binding: &CaptureBinding,
        progress: &CaptureProgress,
        _: Instant,
    ) -> Result<RawEvidence, RecoveryError> {
        let extra = u64::from(self.tail);
        Ok(RawEvidence {
            stdout: StreamObservation::Present {
                identity: binding.stdout.clone(),
                length: progress.stdout + extra,
            },
            stderr: StreamObservation::Present {
                identity: binding.stderr.clone(),
                length: progress.stderr,
            },
        })
    }
}

fn id(byte: u8) -> Id {
    Id::new(format!("{byte:02x}").repeat(16)).unwrap()
}
fn digest() -> Digest {
    Digest::new("ab".repeat(32)).unwrap()
}
fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 42, 7, BOOT.into()).unwrap()
}
fn file(inode: u64) -> FileIdentity {
    FileIdentity { device: 1, inode }
}
fn source(generation: Id) -> SourceStamp {
    SourceStamp {
        generation,
        root: file(1),
        database: file(2),
        marker: file(3),
        store_lock: file(4),
        io_lock: file(5),
    }
}
fn capture(binding: CaptureBinding) -> CaptureSnapshot {
    CaptureSnapshot {
        binding,
        progress: CaptureProgress {
            revision: 2,
            stdout: 4,
            stderr: 3,
            stdout_eof: true,
            stderr_eof: true,
            loss: false,
        },
        exit: ProcessExit::Code(0),
    }
}
fn record() -> RecoveryRecord {
    let operation = id(1);
    let generation = id(2);
    let request = PreparedRequest {
        operation: operation.clone(),
        session: id(3),
        reservation: id(4),
        attempt: id(5),
        generation: generation.clone(),
        digest: digest(),
        payload: vec![],
    };
    let owner = OwnerClaim {
        owner: id(6),
        nonce: id(7),
        process: process(111),
    };
    let ownership = Snapshot {
        request,
        phase: Phase::AcceptanceRecorded,
        revision: 3,
        cancelled: false,
        owner: Some(owner.clone()),
        child: Some(process(112)),
    };
    let binding = CaptureBinding {
        fence: Fence {
            operation,
            generation: generation.clone(),
            attempt: id(5),
            owner,
            revision: 3,
        },
        child: process(112),
        stdout: file(10),
        stderr: file(11),
    };
    RecoveryRecord {
        source: source(generation),
        ownership,
        capture: Some(capture(binding)),
        events_count: 2,
        events_digest: digest(),
    }
}
fn inspect(
    tail: bool,
) -> (
    Result<Option<hf_domain::recovery::RecoveryReport>, RecoveryError>,
    u8,
) {
    let reads = Rc::new(Cell::new(0));
    let store = Store(record(), reads.clone());
    let inspector = RecoveryInspector::new(store, Identity, Raw { tail });
    let result = inspector.inspect(&id(1), Instant::now() + Duration::from_secs(1));
    (result, reads.get())
}

#[test]
fn positively_absent_owner_does_not_change_terminal_or_reservation_evidence() {
    let (result, reads) = inspect(false);
    let report = result.unwrap().unwrap();
    assert_eq!(reads, 2, "storage snapshot is revalidated");
    assert_eq!(report.owner.unwrap().state, ProcessState::AbsentPid);
    assert_eq!(
        report.capture,
        CaptureAssessment::RecordedTerminalExtentObserved
    );
    assert_eq!(report.reservation, ReservationDisposition::Retained);
}

#[test]
fn raw_tail_is_observed_but_never_promoted_to_synced_or_terminal_capture() {
    let (result, reads) = inspect(true);
    let report = result.unwrap().unwrap();
    assert_eq!(reads, 2);
    assert_eq!(report.capture, CaptureAssessment::UnindexedTail);
    assert_eq!(report.reservation, ReservationDisposition::Retained);
}

#[path = "recovery/models.rs"]
mod models;
