//! Owner evidence for a state recovery: which recorded processes could still own the
//! root, and how current the rows that name them are.
//!
//! The storage adapter reads the rows under the maintenance session's exclusive locks and
//! returns an `EvidenceSet`; `assess` judges it with an injected `IdentityPort`. Alive is
//! `ActiveOwner`, undecidable is `AmbiguousOwner`, and only a dead identity passes.
use crate::process::IdentityPort;
use hf_domain::process::{IdentityObservation, ProcessIdentity};
use std::time::Duration;

/// Deadline of the private copy of the main file, when `-wal` is missing.
pub const COPY_BUDGET: Duration = Duration::from_secs(60);
/// Deadline of the owner-evidence queries.
pub const QUERY_BUDGET: Duration = Duration::from_secs(5);

/// How current the rows are that the evidence was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Basis {
    /// The rows were read and qualified, and nothing says the store is behind.
    Clear,
    /// The rows were read and qualified, but the store is behind its acknowledgements:
    /// `CommitLost`, a watermark that is absent or invalid, an acceptance record that is
    /// not `accepted`, or a lost `-wal` (read through a private copy).
    EarlierSnapshot,
    /// The rows could not be read or qualified: marker, header, identity or schema
    /// mismatch, a SQLite error, a failed copy, or a deadline.
    Unreadable,
}
impl Basis {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::EarlierSnapshot => "earlier_snapshot",
            Self::Unreadable => "unreadable",
        }
    }
}

/// The row family an item came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// `ownership_records.owner_process`, in any phase.
    OwnershipOwner,
    /// `ownership_records.child`, in any phase.
    OwnershipChild,
    /// `task_processes`.
    TaskProcess,
    /// A task record that cannot establish its own state.
    TaskRecord,
    /// A launch attempt that may have spawned a process without recording its identity.
    LaunchAttempt,
}

/// What one item asks of the identity port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subject {
    /// A process identity as stored.
    Identity(ProcessIdentity),
    /// A recorded pid with no start evidence: it can never be tied to a process, so it is
    /// always undecidable (and is not journaled).
    Pid(u32),
    /// Evidence that is missing or unqualifiable by construction (a non-terminal task with
    /// no process row, a claim without a child, a damaged task record, a launch attempt
    /// without an identity).
    Undecidable,
}

/// One checked item of the evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub source: Source,
    pub subject: Subject,
}

/// What the storage read produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceSet {
    pub basis: Basis,
    pub items: Vec<Item>,
}

/// Why a recovery may not proceed on the evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A recorded process is alive (reason `active_owner`).
    ActiveOwner,
    /// A recorded process cannot be judged (reason `ambiguous_owner`).
    AmbiguousOwner,
}

/// What a recovery journals: the basis and every identity that was checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerEvidence {
    pub basis: Basis,
    pub checked: Vec<ProcessIdentity>,
}

/// Why the evidence could not be read at all (distinct from an `Unreadable` basis, which
/// is a readable verdict about the data).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceError {
    /// The session's root changed under it.
    Stale,
    /// A filesystem or lock failure outside the data.
    Unavailable,
}

/// Reads the owner evidence of the root a maintenance session holds exclusively.
pub trait OwnerEvidencePort {
    fn read(&self, operation_id: &str) -> Result<EvidenceSet, EvidenceError>;
}

/// Judges `set`: an `Unreadable` basis is `AmbiguousOwner` whatever the items say (the
/// rows could not be read, so nothing proves no owner); otherwise every identity is observed, a living one is `ActiveOwner` even when other
/// items are undecidable, any undecidable item is `AmbiguousOwner`, and a set whose items
/// are all dead passes with the identities it checked.
pub fn assess(set: &EvidenceSet, port: &dyn IdentityPort) -> Result<OwnerEvidence, Refusal> {
    if set.basis == Basis::Unreadable {
        return Err(Refusal::AmbiguousOwner);
    }
    let mut checked = Vec::new();
    let mut undecidable = false;
    for item in &set.items {
        match &item.subject {
            Subject::Identity(identity) => match verdict(port.observe(identity)) {
                Verdict::Alive => return Err(Refusal::ActiveOwner),
                Verdict::Undecidable => undecidable = true,
                Verdict::Dead => checked.push(identity.clone()),
            },
            Subject::Pid(_) | Subject::Undecidable => undecidable = true,
        }
    }
    if undecidable {
        return Err(Refusal::AmbiguousOwner);
    }
    Ok(OwnerEvidence {
        basis: set.basis,
        checked,
    })
}

/// A resume's check: only the identities the journal recorded, with no database read.
pub fn recheck(checked: &[ProcessIdentity], port: &dyn IdentityPort) -> Result<(), Refusal> {
    let mut undecidable = false;
    for identity in checked {
        match verdict(port.observe(identity)) {
            Verdict::Alive => return Err(Refusal::ActiveOwner),
            Verdict::Undecidable => undecidable = true,
            Verdict::Dead => {}
        }
    }
    if undecidable {
        Err(Refusal::AmbiguousOwner)
    } else {
        Ok(())
    }
}

enum Verdict {
    Alive,
    Dead,
    Undecidable,
}

/// A match is alive; a vanished process or one whose pid now carries another identity is
/// dead; an observation that decides neither is undecidable.
fn verdict(observation: IdentityObservation) -> Verdict {
    match observation {
        IdentityObservation::Matching => Verdict::Alive,
        IdentityObservation::Dead | IdentityObservation::Mismatch => Verdict::Dead,
        IdentityObservation::Unknown => Verdict::Undecidable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const BOOT: &str = "abcdef01-2345-6789-abcd-ef0123456789";

    struct Port(HashMap<u32, IdentityObservation>);
    impl IdentityPort for Port {
        fn identity(&self, _: u32) -> Option<ProcessIdentity> {
            None
        }
        fn observe(&self, expected: &ProcessIdentity) -> IdentityObservation {
            self.0
                .get(&expected.pid())
                .copied()
                .unwrap_or(IdentityObservation::Dead)
        }
    }

    fn id(pid: u32) -> ProcessIdentity {
        ProcessIdentity::new(pid, 10, 0, BOOT.into()).unwrap()
    }
    fn item(pid: u32) -> Item {
        Item {
            source: Source::TaskProcess,
            subject: Subject::Identity(id(pid)),
        }
    }
    fn set(items: Vec<Item>) -> EvidenceSet {
        EvidenceSet {
            basis: Basis::Clear,
            items,
        }
    }

    #[test]
    fn dead_and_mismatched_pass_and_are_recorded() {
        let port = Port(HashMap::from([(5, IdentityObservation::Mismatch)]));
        let result = assess(&set(vec![item(5), item(6)]), &port).unwrap();
        assert_eq!(result.checked, vec![id(5), id(6)]);
    }

    #[test]
    fn unreadable_basis_is_ambiguous_and_pid_only_is_always_undecidable() {
        let port = Port(HashMap::new());
        let unreadable = EvidenceSet {
            basis: Basis::Unreadable,
            items: Vec::new(),
        };
        assert_eq!(assess(&unreadable, &port), Err(Refusal::AmbiguousOwner));
        let pid = Item {
            source: Source::TaskProcess,
            subject: Subject::Pid(77),
        };
        assert_eq!(assess(&set(vec![pid]), &port), Err(Refusal::AmbiguousOwner));
    }

    #[test]
    fn alive_beats_undecidable() {
        let port = Port(HashMap::from([
            (5, IdentityObservation::Unknown),
            (6, IdentityObservation::Matching),
        ]));
        assert_eq!(
            assess(&set(vec![item(5), item(6)]), &port),
            Err(Refusal::ActiveOwner)
        );
        assert_eq!(
            assess(&set(vec![item(5)]), &port),
            Err(Refusal::AmbiguousOwner)
        );
        assert_eq!(recheck(&[id(5)], &port), Err(Refusal::AmbiguousOwner));
        assert_eq!(recheck(&[id(6)], &port), Err(Refusal::ActiveOwner));
        assert_eq!(recheck(&[id(7)], &port), Ok(()));
    }
}
