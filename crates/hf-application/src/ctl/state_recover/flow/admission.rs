use super::*;
use crate::ctl::{
    state_inspect::recovery_eligible,
    state_recover::{
        binding::{BindingError, BindingMode, verify_binding},
        evidence,
        journal::{JOURNAL_BOUND, JournalPhase},
    },
};
use hf_domain::ctl::{
    identity::PRODUCT_VERSION,
    inspect::{FileState, JournalDecode, MarkerDecode},
    recovery_proposal::{INVENTORY_BOUND, Inventory, InventoryFailure},
    state_recovery::{StoreRecovery, is_operation_id},
};

pub(super) fn admit(
    r: &RecoveryRequest<'_>,
    p: &RecoveryPorts<'_>,
) -> Result<(RecoveryJournal, bool), RecoveryError> {
    // Layout-2 refusal precedes confirmation errors. This bounded no-follow
    // observation grants no authority; every effect still needs exact held binding.
    let facts = p.observer.observe(r.root);
    let marker = match &facts.python_marker {
        FileState::Bytes(bytes) => Some(p.state_codec.decode_marker(bytes)),
        _ => None,
    };
    let layout_two = matches!(&marker, Some(MarkerDecode::Store { observed, .. }) if observed.iter().any(|field| field.key == "layout_version" && field.value == hf_domain::ctl::inspect::Scalar::Int(2)));
    if layout_two {
        return Err(RecoveryError::UnsupportedLayout);
    }
    if !is_operation_id(r.operation_id) {
        return Err(RecoveryError::StaleConfirmation);
    }
    binding(r, p, BindingMode::Fresh)?;
    if matches!(marker, Some(MarkerDecode::Store { .. })) {
        return Err(RecoveryError::UnsafeRoot);
    }
    let selected = if facts.journal_pending == FileState::Absent {
        &facts.journal
    } else {
        &facts.journal_pending
    };
    let mut previous = None;
    if let FileState::Bytes(bytes) = selected {
        if bytes.len() > JOURNAL_BOUND {
            return Err(RecoveryError::StaleConfirmation);
        }
        match p.state_codec.decode_journal(bytes, r.root) {
            JournalDecode::Qualified {
                operation_id,
                completed: true,
                proposal: Some(old),
                ..
            } if operation_id != r.operation_id
                && old.engine_version == PRODUCT_VERSION
                && p.state_codec.journal_engine_version(bytes).as_deref()
                    == Some(PRODUCT_VERSION) =>
            {
                // A previous journal is preserved only from the final file, and only
                // after the canonical state codec qualified its full document.
                if facts.journal_pending != FileState::Absent {
                    return Err(RecoveryError::StaleConfirmation);
                }
                previous = Some(bytes.clone());
            }
            _ => {
                return resume(r, p, &facts, bytes)
                    .map(|j| (j, false))
                    .map_err(|error| {
                        if error == RecoveryError::InfrastructureUnavailable {
                            RecoveryError::RecoveryInterrupted
                        } else {
                            error
                        }
                    });
            }
        }
    } else if *selected != FileState::Absent {
        return Err(RecoveryError::RecoveryInterrupted);
    }
    if !recovery_eligible(&facts, p.state_codec, p.policy, r.root, false) {
        return Err(RecoveryError::StaleConfirmation);
    }
    fresh(r, p, previous).map(|j| (j, true))
}

fn fresh(
    r: &RecoveryRequest<'_>,
    p: &RecoveryPorts<'_>,
    previous: Option<Vec<u8>>,
) -> Result<RecoveryJournal, RecoveryError> {
    p.session.verify_held().map_err(binding_error)?;
    let inventory = p
        .inventory
        .walk(r.root, INVENTORY_BOUND)
        .map_err(inventory_error)?;
    if inventory.has_reference_state() {
        return Err(RecoveryError::UnsafeRoot);
    }
    if inventory.root != p.session.held_root_identity()
        || inventory.lock != Some(p.session.held_lock_identity())
    {
        return Err(RecoveryError::StaleConfirmation);
    }
    check_digests(&inventory, previous.as_deref(), r.proposal, p)?;
    p.mutations.staging_absent(r.operation_id)?;
    p.mutations.backup_absent(&r.proposal.backup_path())?;
    let set = p.evidence.read(r.operation_id).map_err(|e| match e {
        evidence::EvidenceError::Stale => RecoveryError::StaleConfirmation,
        evidence::EvidenceError::Unavailable => RecoveryError::InfrastructureUnavailable,
    })?;
    let owner_evidence = evidence::assess(&set, p.identities).map_err(owner_error)?;
    binding(r, p, BindingMode::Fresh)?;
    let current = p.observer.observe(r.root);
    let previous_matches = match (&current.journal, previous.as_deref()) {
        (FileState::Absent, None) => true,
        (FileState::Bytes(actual), Some(expected)) => actual == expected,
        _ => false,
    };
    if current.root_id != Some(p.session.held_root_identity())
        || !recovery_eligible(&current, p.state_codec, p.policy, r.root, false)
        || current.journal_pending != FileState::Absent
        || !previous_matches
    {
        return Err(RecoveryError::StaleConfirmation);
    }
    let journal = RecoveryJournal {
        engine_version: PRODUCT_VERSION.into(),
        state_root: r.root.as_str().into(),
        operation_id: r.operation_id.into(),
        proposal: r.proposal.clone(),
        backup_path: r.proposal.backup_path(),
        root_identity: p.session.held_root_identity(),
        lock_identity: p.session.held_lock_identity(),
        inventory,
        created_at: r.created_at.into(),
        phase: JournalPhase::Planned,
        backup_identity: None,
        data_identity: None,
        previous_journal_bytes: previous,
        owner_evidence,
        staging: None,
        staged: None,
        result: None,
    };
    preflight(&journal, p)?;
    Ok(journal)
}

fn resume(
    r: &RecoveryRequest<'_>,
    p: &RecoveryPorts<'_>,
    facts: &hf_domain::ctl::inspect::RootFacts,
    bytes: &[u8],
) -> Result<RecoveryJournal, RecoveryError> {
    if !recovery_eligible(facts, p.state_codec, p.policy, r.root, true) {
        return Err(RecoveryError::StaleConfirmation);
    }
    let FileState::Bytes(record) = &facts.recovery_record else {
        return Err(RecoveryError::RecoveryInterrupted);
    };
    if !StoreRecovery::decode(record)
        .is_some_and(|record| record.is_pending() && record.operation_id() == r.operation_id)
    {
        return Err(RecoveryError::RecoveryInterrupted);
    }
    let journal = p
        .journal_codec
        .decode(bytes)
        .map_err(|_| RecoveryError::RecoveryInterrupted)?;
    if journal.proposal != *r.proposal {
        return Err(RecoveryError::StaleConfirmation);
    }
    binding(
        r,
        p,
        BindingMode::Resume {
            lock_identity: journal.lock_identity,
        },
    )?;
    check_digests(
        &journal.inventory,
        journal.previous_journal_bytes.as_deref(),
        r.proposal,
        p,
    )?;
    bounded(&journal, p)?;
    evidence::recheck(&journal.owner_evidence.checked, p.identities).map_err(owner_error)?;
    Ok(journal)
}

fn check_digests(
    inventory: &Inventory,
    previous: Option<&[u8]>,
    proposal: &RecoveryProposal,
    p: &RecoveryPorts<'_>,
) -> Result<(), RecoveryError> {
    let mut comparable = inventory.clone();
    comparable.lock = proposal.lock_identity;
    let digest = |b: &[u8]| p.digest.digest_parts(&[b]).as_str().to_string();
    if digest(&p.inventory_codec.inventory_bytes(&comparable)) != proposal.inventory_sha256
        || previous.is_some_and(|b| b.len() > JOURNAL_BOUND)
        || previous.map(digest) != proposal.prior_journal_sha256
    {
        return Err(RecoveryError::StaleConfirmation);
    }
    Ok(())
}
pub(super) fn bounded(
    j: &RecoveryJournal,
    p: &RecoveryPorts<'_>,
) -> Result<Vec<u8>, RecoveryError> {
    j.validate()
        .map_err(|_| RecoveryError::RecoveryInterrupted)?;
    let bytes = p
        .journal_codec
        .encode(j)
        .map_err(|_| RecoveryError::InfrastructureUnavailable)?;
    if bytes.len() > JOURNAL_BOUND {
        return Err(RecoveryError::InfrastructureUnavailable);
    }
    Ok(bytes)
}
fn binding(
    r: &RecoveryRequest<'_>,
    p: &RecoveryPorts<'_>,
    mode: BindingMode,
) -> Result<(), RecoveryError> {
    verify_binding(
        p.session,
        r.proposal,
        r.root.as_str(),
        PRODUCT_VERSION,
        r.operation_id,
        mode,
    )
    .map_err(binding_error)
}
pub(super) fn binding_error(e: BindingError) -> RecoveryError {
    match e {
        BindingError::Stale => RecoveryError::StaleConfirmation,
        BindingError::ActiveOwner => RecoveryError::ActiveOwner,
        BindingError::Unavailable => RecoveryError::InfrastructureUnavailable,
    }
}
pub(super) fn owner_error(e: evidence::Refusal) -> RecoveryError {
    match e {
        evidence::Refusal::ActiveOwner => RecoveryError::ActiveOwner,
        evidence::Refusal::AmbiguousOwner => RecoveryError::AmbiguousOwner,
    }
}

fn preflight(journal: &RecoveryJournal, p: &RecoveryPorts<'_>) -> Result<(), RecoveryError> {
    bounded(journal, p)?;
    // Reserve the largest identities and longest supported phase spelling before the
    // first mutation; later phase journals must fit the same bounded codec.
    let mut maximum = journal.clone();
    let identity = hf_domain::ctl::recovery_proposal::FileIdentity {
        dev: i64::MIN,
        ino: u64::MAX,
    };
    maximum.backup_identity = Some(identity);
    maximum.data_identity = Some(identity);
    for phase in [JournalPhase::Prepared, JournalPhase::BackedUp] {
        maximum.phase = phase;
        bounded(&maximum, p)?;
    }
    maximum.staging = Some(identity);
    bounded(&maximum, p)?;
    maximum.staged = Some(
        super::super::journal::FRESH_FILES
            .iter()
            .map(|name| {
                (
                    (*name).to_string(),
                    super::super::journal::StagedFile {
                        identity,
                        size: u64::MAX,
                        mtime_ns: i64::MIN,
                        sha256: "f".repeat(64),
                    },
                )
            })
            .collect(),
    );
    bounded(&maximum, p)?;
    maximum.result = Some(super::super::journal::RecoveryResult {
        operation_id: maximum.operation_id.clone(),
        state_root: maximum.state_root.clone(),
        backup_path: maximum.backup_path.clone(),
    });
    maximum.phase = JournalPhase::Completed;
    bounded(&maximum, p)?;
    Ok(())
}

fn inventory_error(error: InventoryFailure) -> RecoveryError {
    match error {
        InventoryFailure::Changed => RecoveryError::StaleConfirmation,
        InventoryFailure::Special => RecoveryError::UnsafeRoot,
        InventoryFailure::TooLarge
        | InventoryFailure::Unreadable
        | InventoryFailure::Unsupported => RecoveryError::InfrastructureUnavailable,
    }
}
