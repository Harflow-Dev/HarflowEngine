#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub(crate) use hf_application::owner_runtime::{CoordinatorError, StorageCoordinator};
pub(crate) use hf_domain::{
    capture::{CaptureBinding, CaptureProgress, CaptureSnapshot, FileIdentity},
    owner_runtime::{
        AckDisposition, CommandIdentity, RawIdentities, StorageAck, StorageBootstrap,
        StorageCommand, StorageParticipant, Stream, TrustedDrainEvidence,
    },
    ownership::{Digest, Fence, Id, OwnerClaim, Phase, PreparedRequest, Snapshot},
    process::{ProcessExit, ProcessIdentity},
    recovery::SourceStamp,
};
pub(crate) use std::time::{Duration, Instant};

pub(crate) fn id(c: char) -> Id {
    Id::new(c.to_string().repeat(32)).unwrap()
}
pub(crate) fn digest(c: char) -> Digest {
    Digest::new(c.to_string().repeat(64)).unwrap()
}
pub(crate) fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 100, 0, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}
pub(crate) fn prepared() -> PreparedRequest {
    PreparedRequest {
        operation: id('1'),
        session: id('2'),
        reservation: id('3'),
        attempt: id('4'),
        generation: id('5'),
        digest: digest('a'),
        payload: b"fixed preparation".to_vec(),
    }
}
pub(crate) fn owner() -> OwnerClaim {
    OwnerClaim {
        owner: id('6'),
        nonce: id('7'),
        process: process(41),
    }
}
pub(crate) fn coordinator() -> StorageCoordinator {
    StorageCoordinator::new(prepared(), owner(), digest('b'))
}
pub(crate) fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(1)
}
pub(crate) fn bootstrap() -> StorageCommand {
    let request = prepared();
    let owner = owner();
    let file = FileIdentity {
        device: 1,
        inode: 2,
    };
    StorageCommand::Bootstrap(Box::new(StorageBootstrap {
        root: "/isolated/fixture".into(),
        participant: StorageParticipant {
            source: SourceStamp {
                generation: request.generation.clone(),
                root: file.clone(),
                database: file.clone(),
                marker: file.clone(),
                store_lock: file.clone(),
                io_lock: file,
            },
            operation: request.operation.clone(),
            attempt: request.attempt.clone(),
            request_digest: request.digest.clone(),
            configuration_digest: digest('c'),
            profile_digest: digest('d'),
            owner,
            writer: process(42),
            channel_nonce: id('8'),
        },
        request,
        gate: None,
    }))
}
pub(crate) fn prepared_snapshot() -> Snapshot {
    Snapshot {
        request: prepared(),
        phase: Phase::Prepared,
        revision: 0,
        cancelled: false,
        owner: None,
        child: None,
    }
}
pub(crate) fn ack(
    identity: CommandIdentity,
    revision: u64,
    snapshot: Snapshot,
    capture: Option<CaptureSnapshot>,
    raw_files: Option<RawIdentities>,
) -> StorageAck {
    StorageAck {
        identity,
        disposition: AckDisposition::Committed,
        revision: Some(revision),
        snapshot: Some(snapshot),
        capture,
        raw_files,
    }
}
pub(crate) fn commit(
    owner: &mut StorageCoordinator,
    command: StorageCommand,
    revision: u64,
    snapshot: Snapshot,
    capture: Option<CaptureSnapshot>,
    raw_files: Option<RawIdentities>,
) {
    let expected_raw = if matches!(command, StorageCommand::CreateRaw) {
        raw_files
    } else {
        owner.raw_files().cloned()
    };
    let identity = owner
        .begin(command, digest('c'), deadline(), Instant::now())
        .unwrap()
        .identity
        .clone();
    owner.send_complete(&identity, Instant::now()).unwrap();
    owner
        .acknowledge(
            ack(identity, revision, snapshot, capture, expected_raw),
            Instant::now(),
        )
        .unwrap();
}
pub(crate) fn claimed_snapshot() -> Snapshot {
    Snapshot {
        request: prepared(),
        phase: Phase::Claimed,
        revision: 1,
        cancelled: false,
        owner: Some(owner()),
        child: None,
    }
}
pub(crate) fn spawned_snapshot() -> Snapshot {
    Snapshot {
        request: prepared(),
        phase: Phase::SpawnObserved,
        revision: 2,
        cancelled: false,
        owner: Some(owner()),
        child: Some(process(43)),
    }
}
pub(crate) fn accepted_snapshot() -> Snapshot {
    Snapshot {
        request: prepared(),
        phase: Phase::AcceptanceRecorded,
        revision: 3,
        cancelled: false,
        owner: Some(owner()),
        child: Some(process(43)),
    }
}
pub(crate) fn capture_binding() -> CaptureBinding {
    CaptureBinding {
        fence: Fence {
            operation: id('1'),
            generation: id('5'),
            attempt: id('4'),
            owner: owner(),
            revision: 3,
        },
        child: process(43),
        stdout: raw_files().stdout,
        stderr: raw_files().stderr,
    }
}
pub(crate) fn raw_files() -> RawIdentities {
    RawIdentities {
        stdout: FileIdentity {
            device: 1,
            inode: 4,
        },
        stderr: FileIdentity {
            device: 1,
            inode: 5,
        },
    }
}
pub(crate) fn capture_at(
    binding: &CaptureBinding,
    revision: u64,
    stdout: u64,
    stderr: u64,
    stdout_eof: bool,
    stderr_eof: bool,
    exit: ProcessExit,
) -> CaptureSnapshot {
    CaptureSnapshot {
        binding: binding.clone(),
        progress: CaptureProgress {
            revision,
            stdout,
            stderr,
            stdout_eof,
            stderr_eof,
            loss: false,
        },
        exit,
    }
}
pub(crate) fn established_coordinator() -> (StorageCoordinator, CaptureBinding, Snapshot) {
    let mut owner = coordinator();
    let prepared = prepared_snapshot();
    commit(&mut owner, bootstrap(), 0, prepared, None, None);
    commit(
        &mut owner,
        StorageCommand::Claim,
        1,
        claimed_snapshot(),
        None,
        None,
    );
    commit(
        &mut owner,
        StorageCommand::RecordChild(process(43)),
        2,
        spawned_snapshot(),
        None,
        None,
    );
    commit(
        &mut owner,
        StorageCommand::CreateRaw,
        2,
        spawned_snapshot(),
        None,
        Some(raw_files()),
    );
    let accepted = accepted_snapshot();
    commit(
        &mut owner,
        StorageCommand::Accept,
        3,
        accepted.clone(),
        None,
        None,
    );
    let binding = capture_binding();
    let capture = capture_at(&binding, 0, 0, 0, false, false, ProcessExit::Unobserved);
    commit(
        &mut owner,
        StorageCommand::CaptureBegin(Box::new(binding.clone())),
        0,
        accepted.clone(),
        Some(capture),
        None,
    );
    (owner, binding, accepted)
}

pub(crate) fn spawned_coordinator() -> StorageCoordinator {
    let mut owner = coordinator();
    commit(&mut owner, bootstrap(), 0, prepared_snapshot(), None, None);
    commit(
        &mut owner,
        StorageCommand::Claim,
        1,
        claimed_snapshot(),
        None,
        None,
    );
    commit(
        &mut owner,
        StorageCommand::RecordChild(process(43)),
        2,
        spawned_snapshot(),
        None,
        None,
    );
    owner
}
