//! Fake effects and fixtures shared by the proposal tests of `state-inspect`: observer,
//! inventory walk, digest and operation ids, with the real codecs.
#![allow(
    dead_code,
    unused_imports,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used
)]
pub use hf_application::{
    ctl::state_inspect::{
        InventoryPort, OperationIdPort, ProposalPorts, StateObserverPort, StorePolicy,
        inspect_state,
    },
    digest::DigestPort,
};
pub use hf_domain::{
    ctl::{
        identity::PRODUCT_VERSION,
        inspect::{
            EntryKind, FileState, InspectReason, InspectStatus, RootEntry, RootFacts, RootStat,
            StateInspection,
        },
        recovery_proposal::{
            EntryBody, EntryStat, FileIdentity, INVENTORY_BOUND, Inventory, InventoryEntry,
            InventoryFailure, PHASE_PROPOSED, RecoveryProposal,
        },
        state_root::StateRoot,
    },
    ownership::Digest,
};
pub use hf_interfaces::ctl::{
    recovery_codec::{self, JsonRecoveryCodec},
    render,
    state_codec::JsonStateCodec,
};
pub use serde_json::{Value, json};
pub use std::{cell::Cell, collections::BTreeMap};

pub const ROOT: &str = "/tmp/state";
pub const POLICY: StorePolicy = StorePolicy {
    schema_version: 9,
    application_id: 0x4846_4C57,
};
pub const FRESH_ID: &str = "0123456789abcdef0123456789abcdef";
pub const JOURNAL_ID: &str = "fedcba9876543210fedcba9876543210";
pub const PHASES: [&str; 4] = ["planned", "prepared", "backed_up", "committed"];

pub struct Observer(RootFacts);
impl StateObserverPort for Observer {
    fn observe(&self, _: &StateRoot) -> RootFacts {
        self.0.clone()
    }
}

pub struct Walk {
    result: Result<Inventory, InventoryFailure>,
    calls: Cell<usize>,
    bounds: Cell<usize>,
}
impl InventoryPort for Walk {
    fn walk(&self, _: &StateRoot, bound: usize) -> Result<Inventory, InventoryFailure> {
        self.calls.set(self.calls.get() + 1);
        self.bounds.set(bound);
        self.result.clone()
    }
}

/// Not SHA-256: a deterministic stand-in whose output is a valid 64-digit digest.
pub struct Digests;
pub fn stand_in(parts: &[&[u8]]) -> Digest {
    let mut sum = 0u64;
    let mut length = 0u64;
    for part in parts {
        for byte in *part {
            sum = sum.wrapping_mul(131).wrapping_add(u64::from(*byte) + 1);
            length += 1;
        }
    }
    Digest::new(format!("{sum:016x}{length:016x}{sum:016x}{length:016x}")).unwrap()
}
impl DigestPort for Digests {
    fn digest_parts(&self, parts: &[&[u8]]) -> Digest {
        stand_in(parts)
    }
}

pub struct Ids(Option<&'static str>);
impl OperationIdPort for Ids {
    fn fresh(&self) -> Option<String> {
        self.0.map(str::to_string)
    }
}

pub fn stat(ino: u64) -> EntryStat {
    EntryStat {
        dev: 7,
        ino,
        mode: 0o100_644,
        size: 3,
        mtime_ns: 99,
    }
}

pub fn inventory() -> Inventory {
    Inventory {
        root: FileIdentity { dev: 7, ino: 100 },
        lock: Some(FileIdentity { dev: 7, ino: 101 }),
        entries: BTreeMap::from([(
            "stray".to_string(),
            InventoryEntry {
                stat: stat(5),
                body: EntryBody::File,
            },
        )]),
    }
}

pub fn directory(children: &[(&str, InventoryEntry)]) -> InventoryEntry {
    InventoryEntry {
        stat: stat(50),
        body: EntryBody::Directory(
            children
                .iter()
                .map(|(name, entry)| ((*name).to_string(), entry.clone()))
                .collect(),
        ),
    }
}

pub fn file() -> InventoryEntry {
    InventoryEntry {
        stat: stat(60),
        body: EntryBody::File,
    }
}

/// An existing root holding one stray file: `incompatible`, `unmarked_nonempty`.
pub fn stray_facts() -> RootFacts {
    let mut facts = RootFacts::without_files(RootStat::Present);
    facts.entries = vec![RootEntry {
        name: "stray".to_string(),
        kind: EntryKind::File,
        empty_directory: false,
    }];
    facts
}

pub struct Case {
    pub facts: RootFacts,
    pub walk: Result<Inventory, InventoryFailure>,
    pub ids: Option<&'static str>,
}
impl Case {
    pub fn new(mut facts: RootFacts) -> Self {
        if facts.stat == RootStat::Present && facts.root_id.is_none() {
            facts.root_id = Some(inventory().root);
        }
        Self {
            facts,
            walk: Ok(inventory()),
            ids: Some(FRESH_ID),
        }
    }
    pub fn stray() -> Self {
        Self::new(stray_facts())
    }
    pub fn walking(mut self, result: Result<Inventory, InventoryFailure>) -> Self {
        self.walk = result;
        self
    }
    pub fn with(mut self, change: impl FnOnce(&mut RootFacts)) -> Self {
        change(&mut self.facts);
        self
    }
    /// The inspection and the number of inventory walks it took.
    pub fn run(self) -> (StateInspection, usize) {
        let walk = Walk {
            result: self.walk,
            calls: Cell::new(0),
            bounds: Cell::new(0),
        };
        let ports = ProposalPorts {
            inventory: &walk,
            codec: &JsonRecoveryCodec,
            digest: &Digests,
            ids: &Ids(self.ids),
        };
        let root = StateRoot::from_normalized(ROOT.to_string());
        let inspection = inspect_state(
            &Observer(self.facts),
            &JsonStateCodec,
            POLICY,
            &ports,
            &root,
        );
        if walk.calls.get() > 0 {
            assert_eq!(
                walk.bounds.get(),
                INVENTORY_BOUND,
                "the walk uses the bound"
            );
        }
        (inspection, walk.calls.get())
    }
    pub fn inspect(self) -> StateInspection {
        self.run().0
    }
}

pub fn verdict(inspection: &StateInspection) -> (InspectStatus, InspectReason) {
    (inspection.status, inspection.reason)
}

pub fn offered(inspection: &StateInspection) -> &hf_domain::ctl::recovery_proposal::RecoveryOffer {
    inspection
        .recovery
        .as_deref()
        .unwrap_or_else(|| panic!("a proposal was expected: {inspection:?}"))
}

pub fn journal_bytes(phase: &str, engine_version: &str) -> Vec<u8> {
    let proposal = json!({
        "state_root": ROOT,
        "engine_version": engine_version,
        "root_identity": [7, 100],
        "lock_identity": null,
        "inventory_sha256": "c".repeat(64),
        "operation_id": JOURNAL_ID,
        "prior_journal_sha256": null,
    });
    let document = json!({
        "product": "harflow",
        "format": "harflow.state-recovery",
        "v": 1,
        "state_root": ROOT,
        "phase": phase,
        "operation_id": JOURNAL_ID,
        "proposal": proposal,
        "backup_path": format!("{ROOT}.harflow-backup-{JOURNAL_ID}"),
        "root_identity": [7, 100],
        "inventory": {},
    });
    let mut bytes = serde_json::to_vec(&document).unwrap();
    bytes.push(b'\n');
    bytes
}

pub fn journal_proposal(engine_version: &str) -> RecoveryProposal {
    RecoveryProposal {
        state_root: ROOT.to_string(),
        engine_version: engine_version.to_string(),
        root_identity: FileIdentity { dev: 7, ino: 100 },
        lock_identity: None,
        inventory_sha256: "c".repeat(64),
        operation_id: JOURNAL_ID.to_string(),
        prior_journal_sha256: None,
    }
}

pub fn marker(product: &str) -> Vec<u8> {
    format!(
        r#"{{"format":"harflow.store","layout_version":1,"product":"{product}","product_version":"0.0.11","v":1}}"#
    )
    .into_bytes()
}

pub type SetFile = fn(&mut RootFacts, FileState);
pub type ChangeFacts = fn(&mut RootFacts);

pub const RESERVED: [(&str, SetFile); 9] = [
    ("python_marker", |f, s| f.python_marker = s),
    ("store_identity", |f, s| f.store_identity = s),
    ("database_header", |f, s| f.database_header = s),
    ("journal", |f, s| f.journal = s),
    ("journal_pending", |f, s| f.journal_pending = s),
    ("wal_acceptance", |f, s| f.wal_acceptance = s),
    ("fence", |f, s| f.fence = s),
    ("watermark", |f, s| f.watermark = s),
    ("recovery_record", |f, s| f.recovery_record = s),
];
