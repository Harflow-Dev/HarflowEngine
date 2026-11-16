//! Recovery proposals for `state-inspect`, decided from observed facts.
//!
//! A proposal is offered when the root exists and is safe, carries no state of the
//! reference engine, every reserved file was readable, and the classification is neither
//! `ready` nor `missing`. The inventory walk then has to finish within its bound without
//! a change (a special entry, an oversize tree or a change during the walk gives no
//! proposal and leaves the classification as it is). An unfinished journal of this
//! engine yields its own proposal, marked resumable. Nothing here opens a database,
//! takes a lease or creates anything.
use super::{StateCodecPort, Verdict, has_non_regular_infrastructure};
use crate::digest::DigestPort;
use hf_domain::ctl::{
    identity::PRODUCT_VERSION,
    inspect::{FileState, InspectStatus, JournalDecode, MarkerDecode, RootFacts, RootStat},
    recovery_proposal::{
        INVENTORY_BOUND, Inventory, InventoryFailure, PHASE_PROPOSED, RecoveryOffer,
        RecoveryProposal,
    },
    state_root::StateRoot,
};

/// Takes the bounded no-follow inventory of a state root.
pub trait InventoryPort {
    fn walk(&self, root: &StateRoot, bound: usize) -> Result<Inventory, InventoryFailure>;
}
/// The canonical byte form of an inventory, whose digest a proposal carries.
pub trait RecoveryCodecPort {
    fn inventory_bytes(&self, inventory: &Inventory) -> Vec<u8>;
}
/// Fresh operation identifiers: 32 lowercase hexadecimal digits.
pub trait OperationIdPort {
    fn fresh(&self) -> Option<String>;
}
/// What proposals need beyond the observer and the codec.
#[derive(Clone, Copy)]
pub struct ProposalPorts<'a> {
    pub inventory: &'a dyn InventoryPort,
    pub codec: &'a dyn RecoveryCodecPort,
    pub digest: &'a dyn DigestPort,
    pub ids: &'a dyn OperationIdPort,
}

/// The decision for one classified root.
pub(super) enum Outcome {
    /// Report the classification as it is.
    Unchanged,
    Offer(RecoveryOffer),
}

pub(super) fn decide(
    facts: &RootFacts,
    verdict: &Verdict,
    codec: &dyn StateCodecPort,
    ports: &ProposalPorts<'_>,
    root: &StateRoot,
) -> Outcome {
    match verdict.status {
        InspectStatus::Ready | InspectStatus::Missing => return Outcome::Unchanged,
        // A resume takes no inventory walk: the root is mid-recovery, and the reference
        // engine does not walk it either. The facts-only checks still apply.
        InspectStatus::Interrupted => {
            return if base_eligible(facts, codec) {
                resumable(facts, codec, root)
            } else {
                Outcome::Unchanged
            };
        }
        InspectStatus::Incompatible => {}
    }
    if !eligible(facts, codec, root) {
        return Outcome::Unchanged;
    }
    // A walk that fails, or a root with reference-engine state, offers nothing.
    let Ok(inventory) = ports.inventory.walk(root, INVENTORY_BOUND) else {
        return Outcome::Unchanged;
    };
    if inventory.has_reference_state() || !matches_facts(facts, &inventory) {
        return Outcome::Unchanged;
    }
    fresh(facts, &inventory, ports, root).map_or(Outcome::Unchanged, Outcome::Offer)
}

/// The walked directory is the one the facts were read from, and no valid marker has
/// appeared in it since. A proposal is advisory: `state-recover` must re-check eligibility
/// under its own exclusion and must not trust the confirmation token alone.
fn matches_facts(facts: &RootFacts, inventory: &Inventory) -> bool {
    facts.root_id == Some(inventory.root)
        && !(facts.python_marker == FileState::Absent
            && inventory.entries.contains_key(".harflow-store.json"))
}

/// Facts-only checks shared by fresh and resumable offers: the root, the reserved files
/// and the marker.
fn base_eligible(facts: &RootFacts, codec: &dyn StateCodecPort) -> bool {
    if facts.stat != RootStat::Present
        || has_non_regular_infrastructure(facts)
        || reserved(facts).any(|state| *state == FileState::Unreadable)
    {
        return false;
    }
    if let FileState::Bytes(bytes) = &facts.python_marker
        && matches!(codec.decode_marker(bytes), MarkerDecode::Store { .. })
    {
        return false;
    }
    true
}

/// The facts-only part of eligibility: the root, the reserved files and the journals.
fn eligible(facts: &RootFacts, codec: &dyn StateCodecPort, root: &StateRoot) -> bool {
    if !base_eligible(facts, codec) {
        return false;
    }
    let journal = if facts.journal_pending == FileState::Absent {
        &facts.journal
    } else {
        &facts.journal_pending
    };
    match journal {
        FileState::Absent => true,
        FileState::Bytes(bytes) => match codec.decode_journal(bytes, root) {
            JournalDecode::Qualified { proposal, .. } => {
                proposal.is_some_and(|proposal| proposal.engine_version == PRODUCT_VERSION)
            }
            JournalDecode::Unparseable | JournalDecode::Unqualified => false,
        },
        _ => false,
    }
}

fn reserved(facts: &RootFacts) -> impl Iterator<Item = &FileState> {
    [
        &facts.python_marker,
        &facts.store_identity,
        &facts.database_header,
        &facts.journal,
        &facts.journal_pending,
        &facts.wal_acceptance,
        &facts.fence,
        &facts.watermark,
        &facts.recovery_record,
    ]
    .into_iter()
}

/// The unfinished journal's own proposal, only when this engine wrote it.
fn resumable(facts: &RootFacts, codec: &dyn StateCodecPort, root: &StateRoot) -> Outcome {
    let journal = if facts.journal_pending == FileState::Absent {
        &facts.journal
    } else {
        &facts.journal_pending
    };
    let FileState::Bytes(bytes) = journal else {
        return Outcome::Unchanged;
    };
    match codec.decode_journal(bytes, root) {
        JournalDecode::Qualified {
            proposal: Some(proposal),
            phase,
            ..
        } if proposal.engine_version == PRODUCT_VERSION => Outcome::Offer(RecoveryOffer {
            proposal: *proposal,
            phase,
            resumable: true,
        }),
        _ => Outcome::Unchanged,
    }
}

fn fresh(
    facts: &RootFacts,
    inventory: &Inventory,
    ports: &ProposalPorts<'_>,
    root: &StateRoot,
) -> Option<RecoveryOffer> {
    let hex = |bytes: &[u8]| ports.digest.digest_parts(&[bytes]).as_str().to_string();
    let prior_journal_sha256 = match &facts.journal {
        FileState::Absent => None,
        FileState::Bytes(bytes) => Some(hex(bytes)),
        _ => return None,
    };
    Some(RecoveryOffer {
        proposal: RecoveryProposal {
            state_root: root.as_str().to_string(),
            engine_version: PRODUCT_VERSION.to_string(),
            root_identity: inventory.root,
            lock_identity: inventory.lock,
            inventory_sha256: hex(&ports.codec.inventory_bytes(inventory)),
            operation_id: ports.ids.fresh()?,
            prior_journal_sha256,
        },
        phase: PHASE_PROPOSED.to_string(),
        resumable: false,
    })
}
