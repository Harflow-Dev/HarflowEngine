#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::support::*;
use hf_application::ctl::state_recover::{
    completion::{CompletionMutationPort, CompletionPorts, CompletionReadPort, CompletionSnapshot},
    flow::{
        RecoveryError, RecoveryPhase, RecoveryPorts, RecoveryProgress, RecoveryRequest,
        recover_qualified,
    },
    initialization::StagingMutationPort,
    journal::{FRESH_FILES, JournalPhase, RecoveryJournal, RecoveryResult, StagedFile},
};
use hf_domain::ctl::{
    recovery_proposal::FileIdentity,
    state_recovery::{RecoveryState, StoreRecovery},
};
use std::{collections::BTreeMap, sync::Mutex};

struct Read {
    root: FileIdentity,
    record: Option<Vec<u8>>,
    journal: Option<Vec<u8>>,
}
impl CompletionReadPort for Read {
    fn read(&self) -> Result<CompletionSnapshot, RecoveryError> {
        Ok(CompletionSnapshot {
            layout_two: false,
            root_identity: self.root,
            record: self.record.clone(),
            journal: self.journal.clone(),
        })
    }
}

#[derive(Default)]
struct Publication {
    calls: Mutex<Vec<&'static str>>,
}
impl CompletionMutationPort for Publication {
    fn validate_locations(&self, _: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.calls.lock().unwrap().push("validate_locations");
        Ok(())
    }
    fn commit(&self, _: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.calls.lock().unwrap().push("commit");
        Ok(())
    }
    fn write_receipt(&self, _: &RecoveryJournal, _: &[u8]) -> Result<(), RecoveryError> {
        self.calls.lock().unwrap().push("receipt_failed");
        Err(RecoveryError::InfrastructureUnavailable)
    }
}

#[derive(Default)]
struct Staging(Mutex<Vec<&'static str>>);
impl StagingMutationPort for Staging {
    fn validate_archive(&self, _: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.0.lock().unwrap().push("validate_archive");
        Ok(())
    }
    fn cleanup_staging(&self, _: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.0.lock().unwrap().push("cleanup");
        Ok(())
    }
    fn prepare_staging(&self, _: &RecoveryJournal) -> Result<FileIdentity, RecoveryError> {
        self.0.lock().unwrap().push("prepare");
        Ok(FileIdentity { dev: 7, ino: 70 })
    }
    fn initialize_staging(
        &self,
        _: &RecoveryJournal,
    ) -> Result<BTreeMap<String, StagedFile>, RecoveryError> {
        self.0.lock().unwrap().push("initialize");
        Ok(FRESH_FILES
            .iter()
            .enumerate()
            .map(|(index, name)| {
                (
                    (*name).to_string(),
                    StagedFile {
                        identity: FileIdentity {
                            dev: 7,
                            ino: 71 + u64::try_from(index).unwrap(),
                        },
                        size: 8,
                        mtime_ns: 9,
                        sha256: "a".repeat(64),
                    },
                )
            })
            .collect())
    }
    fn publish_staged(&self, _: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.0.lock().unwrap().push("publish");
        Ok(())
    }
    fn validate_initialized(&self, _: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.0.lock().unwrap().push("validate_initialized");
        Ok(())
    }
}

fn committed_read(journal: &RecoveryJournal) -> Read {
    Read {
        root: journal.root_identity,
        record: Some(
            StoreRecovery::new(OP, RecoveryState::Committed)
                .unwrap()
                .encode()
                .into_bytes(),
        ),
        journal: Some(b"journal".to_vec()),
    }
}

fn ports(f: &Fixture) -> RecoveryPorts<'_> {
    RecoveryPorts {
        session: &f.session,
        evidence: &f.evidence,
        observer: &f.facts,
        state_codec: &f.state_codec,
        policy: hf_application::ctl::state_inspect::StorePolicy {
            schema_version: 7,
            application_id: 1,
        },
        inventory: &f.inventory,
        inventory_codec: &f.inventory_codec,
        digest: &f.digest,
        identities: &f.identities,
        journal_codec: &f.journal_codec,
        mutations: &f.mutations,
    }
}

fn request(f: &Fixture) -> RecoveryRequest<'_> {
    RecoveryRequest {
        root: &f.root,
        proposal: &f.proposal,
        operation_id: OP,
        created_at: "unused-on-resume",
    }
}

fn completed_journal(f: &Fixture) -> RecoveryJournal {
    let mut journal = f.run().unwrap().journal;
    journal.phase = JournalPhase::Completed;
    journal.result = Some(RecoveryResult {
        operation_id: OP.into(),
        state_root: ROOT.into(),
        backup_path: f.proposal.backup_path(),
    });
    *f.state_codec.journal.lock().unwrap() = Some((journal.clone(), true));
    *f.journal_codec.journal.lock().unwrap() = Some(journal.clone());
    journal
}

#[test]
fn failed_r9_receipt_reports_committed_progress_without_a_completed_receipt() {
    let f = Fixture::new();
    let read = Read {
        root: ROOT_ID,
        record: None,
        journal: None,
    };
    let publication = Publication::default();
    let staging = Staging::default();
    let completion = CompletionPorts {
        read: &read,
        mutations: &publication,
        staging: &staging,
    };
    let failure = recover_qualified(&request(&f), &ports(&f), &completion).unwrap_err();

    assert_eq!(failure.error, RecoveryError::RecoveryInterrupted);
    assert_eq!(
        failure.progress,
        Some(RecoveryProgress {
            operation_id: OP.into(),
            backup_path: f.proposal.backup_path(),
            phase: RecoveryPhase::Committed,
        })
    );
    // R8's fresh-store validation is on the staging port; the directory-only
    // validation recorded here belongs to R9, after commit and before receipt.
    assert_eq!(
        *publication.calls.lock().unwrap(),
        ["commit", "validate_locations", "receipt_failed"]
    );
    assert_eq!(
        staging.0.lock().unwrap().last(),
        Some(&"validate_initialized")
    );
}

#[test]
fn completed_resume_preserves_completed_progress_and_bypasses_evidence_and_initialization() {
    let mut f = Fixture::new();
    let journal = completed_journal(&f);
    *f.evidence.reads.lock().unwrap() = 0;
    f.facts.facts.recovery_record = hf_domain::ctl::inspect::FileState::Bytes(
        StoreRecovery::new(OP, RecoveryState::Committed)
            .unwrap()
            .encode()
            .into_bytes(),
    );
    let read = committed_read(&journal);
    let publication = Publication::default();
    let staging = Staging::default();
    let completion = CompletionPorts {
        read: &read,
        mutations: &publication,
        staging: &staging,
    };

    let failure = recover_qualified(&request(&f), &ports(&f), &completion).unwrap_err();

    assert_eq!(failure.error, RecoveryError::RecoveryInterrupted);
    assert_eq!(
        failure.progress.as_ref().map(|progress| progress.phase),
        Some(RecoveryPhase::Completed)
    );
    let progress = failure.progress.unwrap();
    assert_eq!(progress.operation_id, OP);
    assert_eq!(progress.backup_path, f.proposal.backup_path());
    assert_eq!(*f.evidence.reads.lock().unwrap(), 0);
    assert!(staging.0.lock().unwrap().is_empty());
    assert_eq!(
        *publication.calls.lock().unwrap(),
        ["validate_locations", "receipt_failed"]
    );
}
