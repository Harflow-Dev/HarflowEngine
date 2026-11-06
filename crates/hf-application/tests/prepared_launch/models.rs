#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_application::owner_runtime::DigestPort;
use hf_domain::{
    capture::{CaptureBinding, CaptureProgress, CaptureSnapshot, FileIdentity},
    fixture::Invitation,
    ownership::{Digest, Fence, Id, OwnerClaim, Phase, PreparedRequest, Snapshot},
    prepared_launch::{CaptureAccepted, RawBytes},
    process::{ProcessExit, ProcessIdentity},
    recovery::{RecoveryRecord, SourceStamp},
};

const STDOUT: &[u8] = br#"{"type":"system","subtype":"init","session_id":"fixture"}"#;

pub struct Hash;
impl DigestPort for Hash {
    fn digest_parts(&self, parts: &[&[u8]]) -> Digest {
        let mut bytes = [0_u8; 32];
        let mut at = 0_usize;
        for part in parts {
            for value in *part {
                bytes[at] = bytes[at].wrapping_mul(31).wrapping_add(*value);
                at = (at + 13) % bytes.len();
            }
        }
        Digest::from_sha256_bytes(bytes)
    }
}
fn id(c: char) -> Id {
    Id::new(c.to_string().repeat(32)).unwrap()
}
fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 100, 4, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}
fn identity(inode: u64) -> FileIdentity {
    FileIdentity { device: 1, inode }
}
pub fn source() -> SourceStamp {
    SourceStamp {
        generation: id('5'),
        root: identity(10),
        database: identity(11),
        marker: identity(12),
        store_lock: identity(13),
        io_lock: identity(14),
    }
}
pub fn fixture() -> (
    Invitation,
    PreparedRequest,
    CaptureAccepted,
    RecoveryRecord,
    RawBytes,
) {
    let bytes = RawBytes {
        stdout: STDOUT.to_vec(),
        stderr: Vec::new(),
    };
    let request = PreparedRequest {
        operation: id('1'),
        session: id('2'),
        reservation: id('3'),
        attempt: id('4'),
        generation: id('5'),
        digest: Hash.digest_parts(&[b"HFPL\x01prepared"]),
        payload: b"HFPL\x01prepared".to_vec(),
    };
    let caller = process(41);
    let invitation = Invitation {
        operation: request.operation.clone(),
        generation: request.generation.clone(),
        digest: request.digest.clone(),
        owner: id('6'),
        nonce: id('7'),
        process: caller.clone(),
        revision: 0,
    };
    let owner = OwnerClaim {
        owner: invitation.owner.clone(),
        nonce: invitation.nonce.clone(),
        process: caller,
    };
    let child = process(42);
    let snapshot = Snapshot {
        request: request.clone(),
        phase: Phase::AcceptanceRecorded,
        revision: 3,
        cancelled: false,
        owner: Some(owner.clone()),
        child: Some(child.clone()),
    };
    let fence = Fence {
        operation: request.operation.clone(),
        generation: request.generation.clone(),
        attempt: request.attempt.clone(),
        owner,
        revision: 3,
    };
    let capture = CaptureSnapshot {
        binding: CaptureBinding {
            fence,
            child,
            stdout: identity(20),
            stderr: identity(21),
        },
        progress: CaptureProgress {
            revision: 1,
            stdout: bytes.stdout.len() as u64,
            stderr: 0,
            stdout_eof: true,
            stderr_eof: true,
            loss: false,
        },
        exit: ProcessExit::Code(0),
    };
    let src = source();
    let receipt = CaptureAccepted {
        invitation: invitation.clone(),
        source: src.clone(),
        ownership: snapshot.clone(),
        capture: capture.clone(),
        stdout_digest: Hash.digest_parts(&[&bytes.stdout]),
        stderr_digest: Hash.digest_parts(&[&bytes.stderr]),
    };
    let record = RecoveryRecord {
        source: src,
        ownership: snapshot,
        capture: Some(capture),
        events_count: 1,
        events_digest: Hash.digest_parts(&[b"event-history"]),
    };
    (invitation, request, receipt, record, bytes)
}
