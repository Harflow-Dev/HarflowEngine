use hf_application::{
    ctl::{
        state_inspect::{
            InventoryPort, RecoveryCodecPort, StateCodecPort, StateObserverPort, StorePolicy,
        },
        state_recover::{
            binding::{BindingError, HeldSessionPort},
            evidence::{Basis, EvidenceError, EvidenceSet, OwnerEvidencePort},
            flow::{
                ArchiveMutationPort, BackedUpHandoff, JournalCodecPort, RecoveryError,
                RecoveryPorts, RecoveryRequest, archive_only,
            },
            journal::{JournalError, RecoveryJournal},
        },
    },
    digest::DigestPort,
    process::IdentityPort,
};
use hf_domain::{
    ctl::{
        identity::PRODUCT_VERSION,
        inspect::{FileState, RootEntry, RootFacts, RootStat},
        recovery_proposal::{
            EntryBody, EntryStat, FileIdentity, Inventory, InventoryEntry, RecoveryProposal,
        },
        state_root::StateRoot,
    },
    ownership::Digest,
    process::{IdentityObservation, ProcessIdentity},
};
use std::sync::Mutex;

pub(super) const ROOT: &str = "/tmp/recovery-flow-root";
pub(super) const OP: &str = "0123456789abcdef0123456789abcdef";
pub(super) const ROOT_ID: FileIdentity = FileIdentity { dev: 7, ino: 10 };
pub(super) const LOCK_ID: FileIdentity = FileIdentity { dev: 7, ino: 11 };

pub(super) struct Session {
    pub(super) root_identity: FileIdentity,
    pub(super) lock_identity: FileIdentity,
}
impl HeldSessionPort for Session {
    fn verify_held(&self) -> Result<(), BindingError> {
        Ok(())
    }
    fn state_root(&self) -> &str {
        ROOT
    }
    fn held_root_identity(&self) -> FileIdentity {
        self.root_identity
    }
    fn held_lock_identity(&self) -> FileIdentity {
        self.lock_identity
    }
}

pub(super) struct Observer {
    pub(super) facts: RootFacts,
}
impl StateObserverPort for Observer {
    fn observe(&self, _: &StateRoot) -> RootFacts {
        self.facts.clone()
    }
}

#[derive(Default)]
pub(super) struct StateCodec {
    pub(super) journal: Mutex<Option<(RecoveryJournal, bool)>>,
    pub(super) marker: Mutex<Option<hf_domain::ctl::inspect::MarkerDecode>>,
}
impl StateCodecPort for StateCodec {
    fn journal_engine_version(&self, _: &[u8]) -> Option<String> {
        self.journal
            .lock()
            .unwrap()
            .as_ref()
            .map(|(journal, _)| journal.engine_version.clone())
    }
    fn decode_marker(&self, _: &[u8]) -> hf_domain::ctl::inspect::MarkerDecode {
        self.marker
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(hf_domain::ctl::inspect::MarkerDecode::Invalid)
    }
    fn decode_journal(&self, _: &[u8], _: &StateRoot) -> hf_domain::ctl::inspect::JournalDecode {
        self.journal.lock().unwrap().as_ref().map_or(
            hf_domain::ctl::inspect::JournalDecode::Unqualified,
            |(journal, completed)| hf_domain::ctl::inspect::JournalDecode::Qualified {
                completed: *completed,
                operation_id: journal.operation_id.clone(),
                phase: journal.phase.as_str().into(),
                proposal: Some(Box::new(journal.proposal.clone())),
            },
        )
    }
}

pub(super) struct InventoryPortFake {
    pub(super) inventory: Inventory,
}
impl InventoryPort for InventoryPortFake {
    fn walk(
        &self,
        _: &StateRoot,
        _: usize,
    ) -> Result<Inventory, hf_domain::ctl::recovery_proposal::InventoryFailure> {
        Ok(self.inventory.clone())
    }
}
pub(super) struct InventoryCodec;
impl RecoveryCodecPort for InventoryCodec {
    fn inventory_bytes(&self, _: &Inventory) -> Vec<u8> {
        b"inventory".to_vec()
    }
}
pub(super) struct DigestFake;
impl DigestPort for DigestFake {
    fn digest_parts(&self, parts: &[&[u8]]) -> Digest {
        if parts.concat() == b"inventory" {
            Digest::new("a".repeat(64)).unwrap()
        } else {
            Digest::new("b".repeat(64)).unwrap()
        }
    }
}
#[derive(Default)]
pub(super) struct Evidence {
    pub(super) reads: Mutex<usize>,
}
impl OwnerEvidencePort for Evidence {
    fn read(&self, _: &str) -> Result<EvidenceSet, EvidenceError> {
        *self.reads.lock().unwrap() += 1;
        Ok(EvidenceSet {
            basis: Basis::Clear,
            items: Vec::new(),
        })
    }
}
#[derive(Default)]
pub(super) struct Identities {
    pub(super) reads: Mutex<usize>,
}
impl IdentityPort for Identities {
    fn identity(&self, _: u32) -> Option<ProcessIdentity> {
        None
    }
    fn observe(&self, _: &ProcessIdentity) -> IdentityObservation {
        *self.reads.lock().unwrap() += 1;
        IdentityObservation::Dead
    }
}

#[derive(Default)]
pub(super) struct JournalCodec {
    pub(super) journal: Mutex<Option<RecoveryJournal>>,
    pub(super) result: Mutex<Option<hf_application::ctl::state_recover::journal::RecoveryResult>>,
}
impl JournalCodecPort for JournalCodec {
    fn encode(&self, journal: &RecoveryJournal) -> Result<Vec<u8>, JournalError> {
        *self.journal.lock().unwrap() = Some(journal.clone());
        Ok(b"journal".to_vec())
    }
    fn decode(&self, _: &[u8]) -> Result<RecoveryJournal, JournalError> {
        self.journal
            .lock()
            .unwrap()
            .clone()
            .ok_or(JournalError::Invalid)
    }
    fn encode_result(
        &self,
        result: &hf_application::ctl::state_recover::journal::RecoveryResult,
    ) -> Result<Vec<u8>, JournalError> {
        *self.result.lock().unwrap() = Some(result.clone());
        Ok(b"result".to_vec())
    }
    fn decode_result(
        &self,
        _: &[u8],
    ) -> Result<hf_application::ctl::state_recover::journal::RecoveryResult, JournalError> {
        self.result
            .lock()
            .unwrap()
            .clone()
            .ok_or(JournalError::Invalid)
    }
}

#[derive(Default)]
pub(super) struct Mutations {
    pub(super) calls: Mutex<Vec<String>>,
    pub(super) fail_at: Mutex<Option<String>>,
}
impl Mutations {
    fn call(&self, name: &str) -> Result<(), RecoveryError> {
        self.calls.lock().unwrap().push(name.to_owned());
        if self.fail_at.lock().unwrap().as_deref() == Some(name) {
            Err(RecoveryError::InfrastructureUnavailable)
        } else {
            Ok(())
        }
    }
    pub(super) fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}
impl ArchiveMutationPort for Mutations {
    fn staging_absent(&self, _: &str) -> Result<(), RecoveryError> {
        self.call("staging_absent")
    }
    fn backup_absent(&self, _: &str) -> Result<(), RecoveryError> {
        self.call("backup_absent")
    }
    fn write_pending(&self, _: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.call("pending")
    }
    fn write_journal(&self, journal: &RecoveryJournal, _: &[u8]) -> Result<(), RecoveryError> {
        self.call(&format!("journal:{}", journal.phase.as_str()))
    }
    fn prepare_backup(
        &self,
        _: &RecoveryJournal,
        _: &dyn JournalCodecPort,
    ) -> Result<(FileIdentity, FileIdentity), RecoveryError> {
        self.call("prepare")?;
        Ok((
            FileIdentity { dev: 7, ino: 20 },
            FileIdentity { dev: 7, ino: 21 },
        ))
    }
    fn validate_locations(
        &self,
        _: &RecoveryJournal,
        final_only: bool,
    ) -> Result<(), RecoveryError> {
        self.call(if final_only {
            "validate_final"
        } else {
            "validate_pre_move"
        })
    }
    fn move_entry(
        &self,
        _: &RecoveryJournal,
        name: &str,
        _: &InventoryEntry,
    ) -> Result<(), RecoveryError> {
        self.call(&format!("move:{name}"))
    }
    fn move_shm(&self, _: &RecoveryJournal) -> Result<(), RecoveryError> {
        self.call("move_shm")
    }
    fn write_manifest(&self, _: &RecoveryJournal, _: &[u8]) -> Result<(), RecoveryError> {
        self.call("manifest")
    }
}

pub(super) struct Fixture {
    pub(super) root: StateRoot,
    pub(super) proposal: RecoveryProposal,
    pub(super) facts: Observer,
    pub(super) inventory: InventoryPortFake,
    pub(super) session: Session,
    pub(super) evidence: Evidence,
    pub(super) state_codec: StateCodec,
    pub(super) inventory_codec: InventoryCodec,
    pub(super) digest: DigestFake,
    pub(super) identities: Identities,
    pub(super) journal_codec: JournalCodec,
    pub(super) mutations: Mutations,
}
impl Fixture {
    pub(super) fn new() -> Self {
        let entry = InventoryEntry {
            stat: EntryStat {
                dev: 7,
                ino: 30,
                mode: 0o100600,
                size: 4,
                mtime_ns: 9,
            },
            body: EntryBody::File,
        };
        let mut entries = std::collections::BTreeMap::new();
        entries.insert("store.identity".into(), entry);
        let inventory = Inventory {
            root: ROOT_ID,
            lock: Some(LOCK_ID),
            entries,
        };
        let mut facts = RootFacts::without_files(RootStat::Present);
        facts.root_id = Some(ROOT_ID);
        facts.entries.push(RootEntry {
            name: "store.identity".into(),
            kind: hf_domain::ctl::inspect::EntryKind::File,
            empty_directory: false,
        });
        let proposal = RecoveryProposal {
            state_root: ROOT.into(),
            engine_version: PRODUCT_VERSION.into(),
            root_identity: ROOT_ID,
            lock_identity: Some(LOCK_ID),
            inventory_sha256: "a".repeat(64),
            operation_id: OP.into(),
            prior_journal_sha256: None,
        };
        Self {
            root: StateRoot::from_normalized(ROOT.into()),
            proposal,
            facts: Observer { facts },
            inventory: InventoryPortFake { inventory },
            session: Session {
                root_identity: ROOT_ID,
                lock_identity: LOCK_ID,
            },
            evidence: Evidence::default(),
            state_codec: StateCodec::default(),
            inventory_codec: InventoryCodec,
            digest: DigestFake,
            identities: Identities::default(),
            journal_codec: JournalCodec::default(),
            mutations: Mutations::default(),
        }
    }
    pub(super) fn run(&self) -> Result<BackedUpHandoff, RecoveryError> {
        self.run_as(OP)
    }
    pub(super) fn run_as(&self, operation_id: &str) -> Result<BackedUpHandoff, RecoveryError> {
        let request = RecoveryRequest {
            root: &self.root,
            proposal: &self.proposal,
            operation_id,
            created_at: "2026-11-16T00:00:00Z",
        };
        let ports = RecoveryPorts {
            session: &self.session,
            evidence: &self.evidence,
            observer: &self.facts,
            state_codec: &self.state_codec,
            policy: StorePolicy {
                schema_version: 7,
                application_id: 1,
            },
            inventory: &self.inventory,
            inventory_codec: &self.inventory_codec,
            digest: &self.digest,
            identities: &self.identities,
            journal_codec: &self.journal_codec,
            mutations: &self.mutations,
        };
        archive_only(&request, &ports)
    }
    pub(super) fn install_resume(&mut self, journal: RecoveryJournal) {
        self.facts.facts.journal = FileState::Bytes(b"journal".to_vec());
        self.facts.facts.recovery_record = FileState::Bytes(
            hf_domain::ctl::state_recovery::StoreRecovery::new(
                OP,
                hf_domain::ctl::state_recovery::RecoveryState::Pending,
            )
            .unwrap()
            .encode()
            .into_bytes(),
        );
        *self.state_codec.journal.lock().unwrap() = Some((journal.clone(), false));
        *self.journal_codec.journal.lock().unwrap() = Some(journal);
    }
}
