#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Values of a recovery proposal: the backup path and the reference-state test. They live
//! beside the interface tests because the domain crate admits no test targets.
use hf_domain::ctl::recovery_proposal::{
    EntryBody, EntryStat, FileIdentity, INVENTORY_BOUND, INVENTORY_EXCLUDED, Inventory,
    InventoryEntry, REFERENCE_STATE_DIRECTORIES, RecoveryProposal, TOKEN_BOUND,
};
use std::collections::BTreeMap;

fn stat() -> EntryStat {
    EntryStat {
        dev: 1,
        ino: 2,
        mode: 0o40_755,
        size: 0,
        mtime_ns: 0,
    }
}

fn entry(body: EntryBody) -> InventoryEntry {
    InventoryEntry { stat: stat(), body }
}

fn inventory(entries: Vec<(&str, InventoryEntry)>) -> Inventory {
    Inventory {
        root: FileIdentity { dev: 1, ino: 1 },
        lock: None,
        entries: entries
            .into_iter()
            .map(|(n, e)| (n.to_string(), e))
            .collect(),
    }
}

fn directory(children: Vec<(&str, InventoryEntry)>) -> InventoryEntry {
    entry(EntryBody::Directory(
        children
            .into_iter()
            .map(|(n, e)| (n.to_string(), e))
            .collect::<BTreeMap<_, _>>(),
    ))
}

#[test]
fn bounds_and_names_are_the_planned_constants() {
    assert_eq!(INVENTORY_BOUND, 100_000);
    assert_eq!(TOKEN_BOUND, 8192);
    assert_eq!(
        REFERENCE_STATE_DIRECTORIES,
        ["tasks", "workflow-runs", "workflow-leases"]
    );
    assert_eq!(
        INVENTORY_EXCLUDED,
        [
            ".harflow-store.lock",
            ".harflow-recovery.json",
            ".harflow-recovery.json.pending",
            "store.lock",
            "io.lock",
            "store.recovery",
            "store.recovery.tmp",
            "store.sqlite3-shm",
        ]
    );
}

#[test]
fn the_backup_path_is_a_sibling_named_after_the_operation() {
    let proposal = RecoveryProposal {
        state_root: "/data/state".to_string(),
        engine_version: "v".to_string(),
        root_identity: FileIdentity { dev: 1, ino: 2 },
        lock_identity: None,
        inventory_sha256: "a".repeat(64),
        operation_id: "0123456789abcdef0123456789abcdef".to_string(),
        prior_journal_sha256: None,
    };
    assert_eq!(
        proposal.backup_path(),
        "/data/state.harflow-backup-0123456789abcdef0123456789abcdef"
    );
}

#[test]
fn reference_state_is_a_nonempty_directory_of_a_reference_name_at_the_top() {
    assert!(!inventory(vec![]).has_reference_state());
    for name in REFERENCE_STATE_DIRECTORIES {
        let with = |body: InventoryEntry| inventory(vec![(name, body)]).has_reference_state();
        assert!(
            with(directory(vec![("x", entry(EntryBody::File))])),
            "{name}"
        );
        assert!(
            with(directory(vec![("sub", directory(vec![]))])),
            "{name}: a nested directory counts"
        );
        assert!(!with(directory(vec![])), "{name}: empty");
        assert!(!with(entry(EntryBody::File)), "{name}: a file");
        assert!(
            !with(entry(EntryBody::Link("elsewhere".to_string()))),
            "{name}: a link is not followed"
        );
    }
}

#[test]
fn other_names_and_nested_reference_names_are_not_reference_state() {
    let other = inventory(vec![
        ("workflows", directory(vec![("x", entry(EntryBody::File))])),
        (
            "workflow-owners",
            directory(vec![("x", entry(EntryBody::File))]),
        ),
        ("Tasks", directory(vec![("x", entry(EntryBody::File))])),
        (
            "outer",
            directory(vec![(
                "tasks",
                directory(vec![("x", entry(EntryBody::File))]),
            )]),
        ),
    ]);
    assert!(!other.has_reference_state());
}
