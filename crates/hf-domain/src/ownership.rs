//! Internal trusted-fixture ownership values; no execution or authentication authority.
use crate::process::{ProcessExit, ProcessIdentity};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Id(String);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Digest(String);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidValue;
fn hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|b| b.is_ascii_hexdigit())
}
impl Id {
    pub fn new(value: String) -> Result<Self, InvalidValue> {
        if hex(&value, 32) {
            Ok(Self(value.to_ascii_lowercase()))
        } else {
            Err(InvalidValue)
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl Digest {
    pub fn new(value: String) -> Result<Self, InvalidValue> {
        if hex(&value, 64) {
            Ok(Self(value.to_ascii_lowercase()))
        } else {
            Err(InvalidValue)
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedRequest {
    pub operation: Id,
    pub session: Id,
    pub reservation: Id,
    pub attempt: Id,
    pub generation: Id,
    pub digest: Digest,
    pub payload: Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Prepared,
    Claimed,
    SpawnObserved,
    AcceptanceRecorded,
    Released,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerClaim {
    pub owner: Id,
    pub nonce: Id,
    pub process: ProcessIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fence {
    pub operation: Id,
    pub generation: Id,
    pub attempt: Id,
    pub owner: OwnerClaim,
    pub revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub request: PreparedRequest,
    pub phase: Phase,
    pub revision: u64,
    pub cancelled: bool,
    pub owner: Option<OwnerClaim>,
    pub child: Option<ProcessIdentity>,
}
impl Snapshot {
    pub fn fence(&self) -> Option<Fence> {
        self.owner.clone().map(|owner| Fence {
            operation: self.request.operation.clone(),
            generation: self.request.generation.clone(),
            attempt: self.request.attempt.clone(),
            owner,
            revision: self.revision,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrepareResult {
    Prepared(Snapshot),
    Existing(Snapshot),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaimResult {
    Granted(Snapshot),
    NoGrant(Snapshot),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureAudit {
    Unknown,
    Lossless,
    KnownLoss,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReleaseProof {
    Unclaimed {
        evidence: Id,
    },
    NeverStarted {
        evidence: Id,
        owner_dead: bool,
    },
    Terminal {
        evidence: Id,
        owner_dead: bool,
        child: ProcessIdentity,
        exit: ProcessExit,
        drain_complete: bool,
        containment_complete: bool,
        capture: CaptureAudit,
    },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::{Digest, Id, InvalidValue};

    #[test]
    fn identifiers_and_digests_enforce_hex_lengths_and_normalize_case() {
        assert_eq!(
            Id::new("ABCDEF0123456789ABCDEF0123456789".into())
                .unwrap()
                .as_str(),
            "abcdef0123456789abcdef0123456789"
        );
        assert_eq!(
            Digest::new("AB".repeat(32)).unwrap().as_str(),
            "ab".repeat(32)
        );
        let invalid_ids = [
            String::new(),
            "0".repeat(31),
            "g".repeat(32),
            "0".repeat(33),
        ];
        for value in invalid_ids {
            assert_eq!(Id::new(value), Err(InvalidValue));
        }
        let invalid_digests = [
            String::new(),
            "0".repeat(63),
            "g".repeat(64),
            "0".repeat(65),
        ];
        for value in invalid_digests {
            assert_eq!(Digest::new(value), Err(InvalidValue));
        }
    }
}
