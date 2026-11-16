#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "state_recover_flow/support.rs"]
mod support;
use hf_application::ctl::state_inspect::StorePolicy;
use hf_application::ctl::state_recover::{
    evidence::{Basis, EvidenceError, EvidenceSet, OwnerEvidencePort},
    flow::{RecoveryError, RecoveryPorts, RecoveryRequest, recover},
    journal::{JournalPhase, RecoveryJournal},
};
use hf_domain::ctl::{
    identity::PRODUCT_VERSION,
    inspect::FileState,
    recovery_proposal::{FileIdentity, RecoveryProposal},
};
use support::*;

#[test]
fn fresh_flow_closes_gate_before_planned_journal_and_moves_identity_first() {
    let f = Fixture::new();
    let result = f.run().unwrap();
    assert_eq!(result.journal.phase, JournalPhase::BackedUp);
    assert_eq!(
        f.mutations.calls().join(","),
        "staging_absent,backup_absent,pending,journal:planned,prepare,journal:prepared,validate_pre_move,move:store.identity,move_shm,validate_final,journal:backed_up,manifest,validate_final"
    );
}

#[test]
fn fresh_missing_lock_is_normalized_for_proposal_but_journal_keeps_held_lock() {
    let mut f = Fixture::new();
    f.proposal.lock_identity = None;
    let result = f.run().unwrap();
    assert_eq!(result.journal.proposal.lock_identity, None);
    assert_eq!(result.journal.lock_identity, LOCK_ID);
    assert_eq!(result.journal.inventory.lock, Some(LOCK_ID));
}

#[test]
fn qualified_earlier_snapshot_is_recorded_without_claiming_clear_evidence() {
    struct Earlier;
    impl OwnerEvidencePort for Earlier {
        fn read(&self, _: &str) -> Result<EvidenceSet, EvidenceError> {
            Ok(EvidenceSet {
                basis: Basis::EarlierSnapshot,
                items: Vec::new(),
            })
        }
    }
    let f = Fixture::new();
    let request = RecoveryRequest {
        root: &f.root,
        proposal: &f.proposal,
        operation_id: OP,
        created_at: "2026-11-16T00:00:00Z",
    };
    let ports = RecoveryPorts {
        session: &f.session,
        evidence: &Earlier,
        observer: &f.facts,
        state_codec: &f.state_codec,
        policy: StorePolicy {
            schema_version: 7,
            application_id: 1,
        },
        inventory: &f.inventory,
        inventory_codec: &f.inventory_codec,
        digest: &f.digest,
        identities: &f.identities,
        journal_codec: &f.journal_codec,
        mutations: &f.mutations,
    };
    let result = recover(&request, &ports).unwrap();
    assert_eq!(result.journal.owner_evidence.basis, Basis::EarlierSnapshot);
    assert_eq!(result.journal.phase, JournalPhase::BackedUp);
}

#[test]
fn claimed_owner_without_child_remains_ambiguous() {
    struct Claimed;
    impl OwnerEvidencePort for Claimed {
        fn read(&self, _: &str) -> Result<EvidenceSet, EvidenceError> {
            Ok(EvidenceSet {
                basis: Basis::Clear,
                items: vec![hf_application::ctl::state_recover::evidence::Item {
                    source: hf_application::ctl::state_recover::evidence::Source::OwnershipOwner,
                    subject: hf_application::ctl::state_recover::evidence::Subject::Undecidable,
                }],
            })
        }
    }
    let f = Fixture::new();
    let request = RecoveryRequest {
        root: &f.root,
        proposal: &f.proposal,
        operation_id: OP,
        created_at: "2026-11-16T00:00:00Z",
    };
    let ports = RecoveryPorts {
        session: &f.session,
        evidence: &Claimed,
        observer: &f.facts,
        state_codec: &f.state_codec,
        policy: StorePolicy {
            schema_version: 7,
            application_id: 1,
        },
        inventory: &f.inventory,
        inventory_codec: &f.inventory_codec,
        digest: &f.digest,
        identities: &f.identities,
        journal_codec: &f.journal_codec,
        mutations: &f.mutations,
    };
    assert_eq!(
        recover(&request, &ports),
        Err(RecoveryError::AmbiguousOwner)
    );
    assert!(f.mutations.calls().iter().all(|call| call != "pending"));
}

#[test]
fn backed_up_resume_revalidates_recorded_state_without_replaying_moves_or_evidence_read() {
    let mut f = Fixture::new();
    let completed_attempt = f.run().unwrap();
    f.install_resume(completed_attempt.journal);
    f.mutations.calls.lock().unwrap().clear();
    let resumed = f.run().unwrap();
    assert_eq!(resumed.journal.phase, JournalPhase::BackedUp);
    assert_eq!(*f.evidence.reads.lock().unwrap(), 1);
    assert_eq!(
        f.mutations.calls().join(","),
        "journal:backed_up,validate_final,manifest"
    );
}

#[test]
fn stale_held_root_identity_refuses_before_pending() {
    let mut f = Fixture::new();
    f.session.root_identity = FileIdentity { dev: 7, ino: 99 };
    assert_eq!(f.run().unwrap_err(), RecoveryError::StaleConfirmation);
    assert!(f.mutations.calls().is_empty());
}

#[test]
fn unsafe_observed_root_refuses_without_archive_mutation() {
    let mut f = Fixture::new();
    f.facts.facts.stat = hf_domain::ctl::inspect::RootStat::Unsafe;
    assert_eq!(f.run().unwrap_err(), RecoveryError::StaleConfirmation);
    assert!(f.mutations.calls().is_empty());
}

#[test]
fn layout_two_precedes_invalid_operation_and_stale_binding() {
    let mut f = Fixture::new();
    f.facts.facts.python_marker = FileState::Bytes(b"layout two".to_vec());
    *f.state_codec.marker.lock().unwrap() = Some(hf_domain::ctl::inspect::MarkerDecode::Store {
        observed: vec![hf_domain::ctl::inspect::ObservedField {
            key: "layout_version",
            value: hf_domain::ctl::inspect::Scalar::Int(2),
        }],
        recovery_operation_id: None,
    });
    f.session.root_identity = FileIdentity { dev: 7, ino: 99 };
    assert_eq!(
        f.run_as("invalid").unwrap_err(),
        RecoveryError::UnsupportedLayout
    );
    assert!(f.mutations.calls().is_empty());
}

#[test]
fn other_reference_engine_marker_is_unsafe_after_valid_binding() {
    let mut f = Fixture::new();
    f.facts.facts.python_marker = FileState::Bytes(b"layout one".to_vec());
    *f.state_codec.marker.lock().unwrap() = Some(hf_domain::ctl::inspect::MarkerDecode::Store {
        observed: vec![hf_domain::ctl::inspect::ObservedField {
            key: "layout_version",
            value: hf_domain::ctl::inspect::Scalar::Int(1),
        }],
        recovery_operation_id: None,
    });
    assert_eq!(f.run().unwrap_err(), RecoveryError::UnsafeRoot);
    assert!(f.mutations.calls().is_empty());
}

#[test]
fn completed_previous_journal_is_preserved_byte_for_byte_when_hash_matches() {
    let mut f = Fixture::new();
    let old = RecoveryJournal {
        engine_version: PRODUCT_VERSION.into(),
        state_root: ROOT.into(),
        operation_id: "fedcba9876543210fedcba9876543210".into(),
        proposal: RecoveryProposal {
            state_root: ROOT.into(),
            engine_version: PRODUCT_VERSION.into(),
            root_identity: ROOT_ID,
            lock_identity: Some(LOCK_ID),
            inventory_sha256: "a".repeat(64),
            operation_id: "fedcba9876543210fedcba9876543210".into(),
            prior_journal_sha256: None,
        },
        backup_path: format!("{ROOT}.harflow-backup-fedcba9876543210fedcba9876543210"),
        root_identity: ROOT_ID,
        lock_identity: LOCK_ID,
        inventory: f.inventory.inventory.clone(),
        created_at: "2026-11-15T00:00:00Z".into(),
        phase: JournalPhase::BackedUp,
        backup_identity: Some(FileIdentity { dev: 7, ino: 40 }),
        data_identity: Some(FileIdentity { dev: 7, ino: 41 }),
        previous_journal_bytes: None,
        owner_evidence: hf_application::ctl::state_recover::evidence::OwnerEvidence {
            basis: Basis::Clear,
            checked: Vec::new(),
        },
    };
    let exact = b"{ \"completed prior journal\" : true }\n".to_vec();
    f.proposal.prior_journal_sha256 = Some("b".repeat(64));
    f.facts.facts.journal = FileState::Bytes(exact.clone());
    *f.state_codec.journal.lock().unwrap() = Some((old, true));
    let result = f.run().unwrap();
    assert_eq!(result.journal.previous_journal_bytes, Some(exact));
}

#[test]
fn oversized_previous_journal_refuses_before_pending() {
    let mut f = Fixture::new();
    let mut old = f.run().unwrap().journal;
    old.operation_id = "fedcba9876543210fedcba9876543210".into();
    old.proposal.operation_id = old.operation_id.clone();
    old.backup_path = old.proposal.backup_path();
    f.mutations.calls.lock().unwrap().clear();
    let oversized = vec![b'x'; hf_application::ctl::state_recover::journal::JOURNAL_BOUND + 1];
    f.proposal.prior_journal_sha256 = Some("b".repeat(64));
    f.facts.facts.journal = FileState::Bytes(oversized);
    *f.state_codec.journal.lock().unwrap() = Some((old, true));
    assert_eq!(f.run().unwrap_err(), RecoveryError::StaleConfirmation);
    assert!(f.mutations.calls().is_empty());
}

#[test]
fn mismatched_previous_journal_digest_refuses_before_pending() {
    let mut f = Fixture::new();
    let mut old = f.run().unwrap().journal;
    old.operation_id = "fedcba9876543210fedcba9876543210".into();
    old.proposal.operation_id = old.operation_id.clone();
    old.backup_path = old.proposal.backup_path();
    f.mutations.calls.lock().unwrap().clear();
    f.proposal.prior_journal_sha256 = Some("c".repeat(64));
    f.facts.facts.journal = FileState::Bytes(b"previous".to_vec());
    *f.state_codec.journal.lock().unwrap() = Some((old, true));
    assert_eq!(f.run().unwrap_err(), RecoveryError::StaleConfirmation);
    assert!(f.mutations.calls().is_empty());
}

#[test]
fn infrastructure_failure_after_pending_is_interrupted_and_never_success() {
    let f = Fixture::new();
    *f.mutations.fail_at.lock().unwrap() = Some("journal:planned".into());
    assert_eq!(f.run().unwrap_err(), RecoveryError::RecoveryInterrupted);
    assert_eq!(
        f.mutations.calls().join(","),
        "staging_absent,backup_absent,pending,journal:planned"
    );
}

#[test]
fn every_archive_mutation_failure_is_reported_as_interrupted() {
    for step in [
        "pending",
        "journal:planned",
        "prepare",
        "journal:prepared",
        "validate_pre_move",
        "move:store.identity",
        "move_shm",
        "validate_final",
        "journal:backed_up",
        "manifest",
    ] {
        let f = Fixture::new();
        *f.mutations.fail_at.lock().unwrap() = Some(step.into());
        assert_eq!(
            f.run().unwrap_err(),
            RecoveryError::RecoveryInterrupted,
            "step {step}"
        );
        assert!(
            f.mutations.calls().iter().any(|call| call == step),
            "step {step} was not reached"
        );
    }
}

#[test]
fn owner_evidence_unreadable_refuses_before_pending_or_archive_mutation() {
    struct Unreadable;
    impl OwnerEvidencePort for Unreadable {
        fn read(&self, _: &str) -> Result<EvidenceSet, EvidenceError> {
            Ok(EvidenceSet {
                basis: Basis::Unreadable,
                items: Vec::new(),
            })
        }
    }
    let f = Fixture::new();
    let request = RecoveryRequest {
        root: &f.root,
        proposal: &f.proposal,
        operation_id: OP,
        created_at: "2026-11-16T00:00:00Z",
    };
    let ports = RecoveryPorts {
        session: &f.session,
        evidence: &Unreadable,
        observer: &f.facts,
        state_codec: &f.state_codec,
        policy: StorePolicy {
            schema_version: 7,
            application_id: 1,
        },
        inventory: &f.inventory,
        inventory_codec: &f.inventory_codec,
        digest: &f.digest,
        identities: &f.identities,
        journal_codec: &f.journal_codec,
        mutations: &f.mutations,
    };
    assert_eq!(
        recover(&request, &ports),
        Err(RecoveryError::AmbiguousOwner)
    );
    assert_eq!(
        f.mutations.calls().join(","),
        "staging_absent,backup_absent"
    );
}
