use hf_application::ctl::state_recover::{
    binding::{BindingError, BindingMode, HeldSessionPort, verify_binding},
    evidence::{Basis, OwnerEvidence},
    journal::{INVENTORY_DEPTH_BOUND, JournalError, JournalPhase, RecoveryJournal},
};
use hf_domain::ctl::{
    identity::PRODUCT_VERSION,
    recovery_proposal::{
        EntryBody, EntryStat, FileIdentity, INVENTORY_BOUND, Inventory, InventoryEntry,
        RecoveryProposal,
    },
};
use std::collections::BTreeMap;

const ROOT: &str = "/tmp/state-root";
const ENGINE: &str = PRODUCT_VERSION;
const OP: &str = "0123456789abcdef0123456789abcdef";

#[derive(Clone, Copy)]
struct Session {
    root: &'static str,
    root_id: FileIdentity,
    lock_id: FileIdentity,
    verification: Result<(), BindingError>,
}
impl HeldSessionPort for Session {
    fn verify_held(&self) -> Result<(), BindingError> {
        self.verification
    }
    fn state_root(&self) -> &str {
        self.root
    }
    fn held_root_identity(&self) -> FileIdentity {
        self.root_id
    }
    fn held_lock_identity(&self) -> FileIdentity {
        self.lock_id
    }
}
fn identity(ino: u64) -> FileIdentity {
    FileIdentity { dev: 7, ino }
}
fn proposal(lock_identity: Option<FileIdentity>) -> RecoveryProposal {
    RecoveryProposal {
        state_root: ROOT.into(),
        engine_version: ENGINE.into(),
        root_identity: identity(10),
        lock_identity,
        inventory_sha256: "a".repeat(64),
        operation_id: OP.into(),
        prior_journal_sha256: None,
    }
}
fn session(lock_id: FileIdentity) -> Session {
    Session {
        root: ROOT,
        root_id: identity(10),
        lock_id,
        verification: Ok(()),
    }
}

#[test]
fn fresh_null_lock_normalizes_but_resume_requires_the_journaled_lock() {
    let lock = identity(11);
    let current = session(lock);
    assert_eq!(
        verify_binding(
            &current,
            &proposal(None),
            ROOT,
            ENGINE,
            OP,
            BindingMode::Fresh
        ),
        Ok(())
    );
    assert_eq!(
        verify_binding(
            &current,
            &proposal(None),
            ROOT,
            ENGINE,
            OP,
            BindingMode::Resume {
                lock_identity: identity(12)
            }
        ),
        Err(BindingError::Stale)
    );
    assert_eq!(
        verify_binding(
            &current,
            &proposal(None),
            ROOT,
            ENGINE,
            OP,
            BindingMode::Resume {
                lock_identity: lock
            }
        ),
        Ok(())
    );
}

#[test]
fn binding_refuses_selected_request_mismatches() {
    let lock = identity(11);
    let current = session(lock);
    let cases = [
        (proposal(Some(lock)), "/tmp/other", ENGINE, OP),
        (proposal(Some(lock)), ROOT, "0.2.0", OP),
        (
            proposal(Some(lock)),
            ROOT,
            ENGINE,
            "fedcba9876543210fedcba9876543210",
        ),
    ];
    for (p, root, version, operation) in cases {
        assert_eq!(
            verify_binding(&current, &p, root, version, operation, BindingMode::Fresh),
            Err(BindingError::Stale)
        );
    }
}

#[test]
fn binding_refuses_proposal_mismatches() {
    let lock = identity(11);
    let current = session(lock);
    let mut wrong_proposal = proposal(Some(lock));
    wrong_proposal.state_root = "/tmp/other".into();
    assert_eq!(
        verify_binding(
            &current,
            &wrong_proposal,
            ROOT,
            ENGINE,
            OP,
            BindingMode::Fresh
        ),
        Err(BindingError::Stale)
    );
    let mut wrong_proposal = proposal(Some(lock));
    wrong_proposal.engine_version = "0.2.0".into();
    assert_eq!(
        verify_binding(
            &current,
            &wrong_proposal,
            ROOT,
            ENGINE,
            OP,
            BindingMode::Fresh
        ),
        Err(BindingError::Stale)
    );
    let mut wrong_proposal = proposal(Some(lock));
    wrong_proposal.operation_id = "fedcba9876543210fedcba9876543210".into();
    assert_eq!(
        verify_binding(
            &current,
            &wrong_proposal,
            ROOT,
            ENGINE,
            OP,
            BindingMode::Fresh
        ),
        Err(BindingError::Stale)
    );
    assert_eq!(
        verify_binding(
            &current,
            &proposal(Some(identity(12))),
            ROOT,
            ENGINE,
            OP,
            BindingMode::Fresh
        ),
        Err(BindingError::Stale)
    );
    let mut wrong_root = proposal(Some(lock));
    wrong_root.root_identity = identity(12);
    assert_eq!(
        verify_binding(&current, &wrong_root, ROOT, ENGINE, OP, BindingMode::Fresh),
        Err(BindingError::Stale)
    );
}

#[test]
fn binding_propagates_session_verification_failure() {
    let lock = identity(11);
    let mut unavailable = session(lock);
    unavailable.verification = Err(BindingError::Unavailable);
    assert_eq!(
        verify_binding(
            &unavailable,
            &proposal(Some(lock)),
            ROOT,
            ENGINE,
            OP,
            BindingMode::Fresh
        ),
        Err(BindingError::Unavailable)
    );
}

fn journal(phase: JournalPhase) -> RecoveryJournal {
    let root = identity(10);
    let lock = identity(11);
    let proposal = proposal(Some(lock));
    RecoveryJournal {
        engine_version: ENGINE.into(),
        state_root: ROOT.into(),
        operation_id: OP.into(),
        backup_path: proposal.backup_path(),
        proposal,
        root_identity: root,
        lock_identity: lock,
        inventory: Inventory {
            root,
            lock: Some(lock),
            entries: BTreeMap::new(),
        },
        created_at: "2026-11-16T00:00:00Z".into(),
        phase,
        backup_identity: (phase != JournalPhase::Planned).then_some(identity(20)),
        data_identity: (phase != JournalPhase::Planned).then_some(identity(21)),
        previous_journal_bytes: None,
        owner_evidence: OwnerEvidence {
            basis: Basis::Clear,
            checked: Vec::new(),
        },
    }
}
fn entry(mode: u32, body: EntryBody) -> InventoryEntry {
    InventoryEntry {
        stat: EntryStat {
            dev: 7,
            ino: 30,
            mode,
            size: 0,
            mtime_ns: 1,
        },
        body,
    }
}
fn nested_inventory(depth: usize) -> BTreeMap<String, InventoryEntry> {
    let mut children = BTreeMap::new();
    for _ in 0..depth {
        let mut parent = BTreeMap::new();
        parent.insert("d".into(), entry(0o040755, EntryBody::Directory(children)));
        children = parent;
    }
    children
}

#[test]
fn journal_phases_and_identity_pairs_are_cross_checked() {
    for phase in [
        JournalPhase::Planned,
        JournalPhase::Prepared,
        JournalPhase::BackedUp,
    ] {
        assert_eq!(journal(phase).validate(), Ok(()));
    }
    let mut malformed = journal(JournalPhase::Prepared);
    malformed.data_identity = None;
    assert_eq!(malformed.validate(), Err(JournalError::Invalid));
    let mut malformed = journal(JournalPhase::Planned);
    malformed.backup_path.push_str("-other");
    assert_eq!(malformed.validate(), Err(JournalError::Invalid));
    let mut unreadable = journal(JournalPhase::Planned);
    unreadable.owner_evidence.basis = Basis::Unreadable;
    assert_eq!(unreadable.validate(), Err(JournalError::Invalid));
    let mut prior_without_bytes = journal(JournalPhase::Planned);
    prior_without_bytes.proposal.prior_journal_sha256 = Some("b".repeat(64));
    assert_eq!(prior_without_bytes.validate(), Err(JournalError::Invalid));
    let mut bytes_without_prior = journal(JournalPhase::Planned);
    bytes_without_prior.previous_journal_bytes = Some(b"prior".to_vec());
    assert_eq!(bytes_without_prior.validate(), Err(JournalError::Invalid));
    let mut paired = journal(JournalPhase::Planned);
    paired.proposal.prior_journal_sha256 = Some("b".repeat(64));
    paired.previous_journal_bytes = Some(b"prior".to_vec());
    assert_eq!(paired.validate(), Ok(()));
}

#[test]
fn inventory_depth_counts_root_children_as_zero_and_validates_mode_and_names() {
    let mut at_limit = journal(JournalPhase::Planned);
    at_limit.inventory.entries = nested_inventory(INVENTORY_DEPTH_BOUND);
    assert_eq!(at_limit.validate(), Ok(()));
    let mut beyond = journal(JournalPhase::Planned);
    beyond.inventory.entries = nested_inventory(INVENTORY_DEPTH_BOUND + 1);
    assert_eq!(beyond.validate(), Err(JournalError::TooLarge));

    for (name, mode, body) in [
        ("file", 0o100644, EntryBody::Directory(BTreeMap::new())),
        ("../escape", 0o100644, EntryBody::File),
        ("", 0o100644, EntryBody::File),
        ("link", 0o120777, EntryBody::Link("bad\0target".into())),
    ] {
        let mut malformed = journal(JournalPhase::Planned);
        malformed
            .inventory
            .entries
            .insert(name.into(), entry(mode, body));
        assert_eq!(malformed.validate(), Err(JournalError::Invalid));
    }
}

#[test]
fn inventory_bound_and_infrastructure_exclusions_apply_only_at_root() {
    for count in [INVENTORY_BOUND, INVENTORY_BOUND + 1] {
        let mut value = journal(JournalPhase::Planned);
        for index in 0..count {
            value
                .inventory
                .entries
                .insert(format!("f{index}"), entry(0o100644, EntryBody::File));
        }
        let expected = if count == INVENTORY_BOUND {
            Ok(())
        } else {
            Err(JournalError::TooLarge)
        };
        assert_eq!(value.validate(), expected);
    }
    let mut root_exclusion = journal(JournalPhase::Planned);
    root_exclusion
        .inventory
        .entries
        .insert("store.lock".into(), entry(0o100644, EntryBody::File));
    assert_eq!(root_exclusion.validate(), Err(JournalError::Invalid));
    let mut nested_allowed = journal(JournalPhase::Planned);
    let mut children = BTreeMap::new();
    children.insert("store.lock".into(), entry(0o100644, EntryBody::File));
    nested_allowed.inventory.entries.insert(
        "nested".into(),
        entry(0o040755, EntryBody::Directory(children)),
    );
    assert_eq!(nested_allowed.validate(), Ok(()));
    let mut own_staging = journal(JournalPhase::Planned);
    own_staging.inventory.entries.insert(
        format!(".harflow-recovery-init-{OP}"),
        entry(0o040755, EntryBody::Directory(BTreeMap::new())),
    );
    assert_eq!(own_staging.validate(), Err(JournalError::Invalid));
    let mut another_staging = journal(JournalPhase::Planned);
    another_staging.inventory.entries.insert(
        ".harflow-recovery-init-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        entry(0o040755, EntryBody::Directory(BTreeMap::new())),
    );
    assert_eq!(another_staging.validate(), Ok(()));
}
