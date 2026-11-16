#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::support::*;
use hf_application::ctl::state_recover::{
    completion::{CompletionMutationPort, CompletionPorts, CompletionReadPort, CompletionSnapshot},
    flow::{RecoveryError, RecoveryPorts, RecoveryRequest, recover},
    initialization::StagingMutationPort,
    journal::{RecoveryJournal, StagedFile},
};
use hf_domain::ctl::{
    recovery_proposal::FileIdentity,
    state_recovery::{RecoveryState, StoreRecovery},
};
use std::{collections::BTreeMap, sync::Mutex};

struct CommittedRead {
    root: FileIdentity,
    record: Vec<u8>,
    journal: Vec<u8>,
}
impl CompletionReadPort for CommittedRead {
    fn read(&self) -> Result<CompletionSnapshot, RecoveryError> {
        Ok(CompletionSnapshot {
            layout_two: false,
            root_identity: self.root,
            record: Some(self.record.clone()),
            journal: Some(self.journal.clone()),
        })
    }
}

#[derive(Default)]
struct CompletionCalls(Mutex<Vec<&'static str>>);
impl CompletionMutationPort for CompletionCalls {
    fn validate_locations(&self, _: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.0.lock().unwrap().push("validate_locations");
        Ok(())
    }
    fn commit(&self, _: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.0.lock().unwrap().push("commit");
        Ok(())
    }
    fn write_receipt(&self, _: &RecoveryJournal, _: &[u8]) -> Result<(), RecoveryError> {
        self.0.lock().unwrap().push("receipt");
        Ok(())
    }
}

struct MustNotInitialize;
impl StagingMutationPort for MustNotInitialize {
    fn validate_archive(&self, _: &RecoveryJournal) -> Result<(), RecoveryError> {
        panic!("committed resume revalidated archive or current store")
    }
    fn cleanup_staging(&self, _: &RecoveryJournal) -> Result<(), RecoveryError> {
        panic!("committed resume cleaned staging")
    }
    fn prepare_staging(&self, _: &RecoveryJournal) -> Result<FileIdentity, RecoveryError> {
        panic!("committed resume created staging")
    }
    fn initialize_staging(
        &self,
        _: &RecoveryJournal,
    ) -> Result<BTreeMap<String, StagedFile>, RecoveryError> {
        panic!("committed resume opened or initialized SQLite")
    }
    fn publish_staged(&self, _: &RecoveryJournal) -> Result<(), RecoveryError> {
        panic!("committed resume inspected staged file attributes")
    }
    fn validate_initialized(&self, _: &RecoveryJournal) -> Result<(), RecoveryError> {
        panic!("committed resume validated current SQLite")
    }
}

#[test]
fn matching_commit_bypasses_current_store_evidence_and_staging_on_resume() {
    let f = Fixture::new();
    let journal = f.run().unwrap().journal;
    let read = CommittedRead {
        root: journal.root_identity,
        record: StoreRecovery::new(OP, RecoveryState::Committed)
            .unwrap()
            .encode()
            .into_bytes(),
        journal: b"journal".to_vec(),
    };
    let completion_calls = CompletionCalls::default();
    let staging = MustNotInitialize;
    let completion = CompletionPorts {
        read: &read,
        mutations: &completion_calls,
        staging: &staging,
    };
    let request = RecoveryRequest {
        root: &f.root,
        proposal: &f.proposal,
        operation_id: OP,
        created_at: "unused-on-resume",
    };
    let ports = RecoveryPorts {
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
    };
    f.mutations.calls.lock().unwrap().clear();

    let result = recover(&request, &ports, &completion).unwrap();

    assert_eq!(result.operation_id, OP);
    assert_eq!(
        *f.evidence.reads.lock().unwrap(),
        1,
        "only the initial archive read ran"
    );
    assert_eq!(f.mutations.calls(), vec!["manifest", "journal:completed"]);
    assert_eq!(
        *completion_calls.0.lock().unwrap(),
        vec!["validate_locations", "receipt"]
    );
}
