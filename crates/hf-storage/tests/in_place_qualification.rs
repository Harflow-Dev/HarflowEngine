#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! In-place qualification: open, probe and recover read the store where it lies and
//! never copy it, so size no longer matters and no scratch directory appears.
mod common;
mod in_place_support;
use common::{default_limits, durable_tree, snapshot_tree};
use hf_storage::{Error, Mutation, ReadSession, Store, probe};
use in_place_support::{MAIN, SHM, WAL, has_snapshot_dir, large_store, later, small_store};
use rusqlite::Connection;
use std::{fs, path::Path};

fn open(root: &Path) -> hf_storage::Result<Store> {
    Store::open(
        root,
        default_limits(),
        later(60),
        common::lease::lease_port(),
    )
}

fn read_session(root: &Path) -> hf_storage::Result<ReadSession> {
    ReadSession::open(
        root,
        default_limits(),
        later(60),
        common::lease::lease_port(),
    )
}

fn counted(root: &Path) -> i64 {
    Connection::open(root.join(MAIN))
        .unwrap()
        .query_row("SELECT COUNT(*) FROM fixture_records", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn a_store_above_the_old_copy_cap_opens_probes_and_recovers() {
    let seeded = large_store();
    let root = &seeded.root;
    let identity = probe(root, common::lease::lease_port()).unwrap().unwrap();
    let store = open(root).unwrap();
    assert_eq!(&identity, store.identity());
    store
        .apply_batch(
            &[Mutation::Put {
                key: "after".into(),
                value: b"v".to_vec(),
            }],
            later(60),
        )
        .unwrap();
    drop(store);
    let recovered = Store::recover(root, later(60), common::lease::lease_port()).unwrap();
    assert!(recovered.complete);
    assert_eq!(counted(root), 12);
    assert!(!has_snapshot_dir(root));
}

#[test]
fn a_large_store_is_readable_by_a_read_session_and_probe_changes_no_durable_byte() {
    let seeded = large_store();
    let before = durable_tree(&seeded.root);
    assert!(
        hf_storage::probe_with_deadline(&seeded.root, later(60), common::lease::lease_port())
            .unwrap()
            .is_some()
    );
    assert!(durable_tree(&seeded.root) == before);
    drop(read_session(&seeded.root).unwrap());
}

/// Zeroes the next-page pointer of one overflow page in the middle of the blob.
fn damage_overflow_page(root: &Path) {
    let mut bytes = fs::read(root.join(MAIN)).unwrap();
    let page = usize::from(u16::from_be_bytes([bytes[16], bytes[17]]));
    let page = if page == 1 { 65_536 } else { page };
    let marked: Vec<usize> = (3..bytes.len() / page)
        .filter(|number| {
            bytes[number * page + 4..(number + 1) * page]
                .iter()
                .all(|b| *b == 0xAB)
        })
        .collect();
    assert!(
        marked.len() > 4,
        "the blob must span several overflow pages"
    );
    let victim = marked[marked.len() / 2] * page;
    bytes[victim..victim + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    fs::write(root.join(MAIN), bytes).unwrap();
    let _ = fs::remove_file(root.join(SHM));
    fs::write(root.join(WAL), b"").unwrap();
}

#[test]
fn a_damaged_non_header_page_opens_but_probe_and_recover_refuse_without_changes() {
    let seeded = small_store();
    let root = &seeded.root;
    {
        let connection = Connection::open(root.join(MAIN)).unwrap();
        connection
            .execute(
                "INSERT INTO fixture_records(key,value) VALUES('blob', ?1)",
                [vec![0xAB_u8; 400_000]],
            )
            .unwrap();
        connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .unwrap();
    }
    damage_overflow_page(root);
    let check: String = Connection::open(root.join(MAIN))
        .unwrap()
        .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
        .unwrap();
    assert_ne!(check, "ok", "the fixture must really be damaged");
    // The plain client above removed the empty `-wal` on close; restore the engine shape.
    let _ = fs::remove_file(root.join(SHM));
    fs::write(root.join(WAL), b"").unwrap();
    let before = durable_tree(root);

    assert!(matches!(
        probe(root, common::lease::lease_port()),
        Err(Error::Incompatible)
    ));
    assert!(durable_tree(root) == before);
    assert!(matches!(
        Store::recover(root, later(10), common::lease::lease_port()),
        Err(Error::Incompatible)
    ));
    assert!(durable_tree(root) == before);
    // Open verifies identity only (owner decision A2), so it accepts the store.
    drop(open(root).unwrap());
    assert!(!has_snapshot_dir(root));
}

#[test]
fn a_missing_wal_refuses_everywhere_and_creates_none() {
    let seeded = small_store();
    let root = &seeded.root;
    fs::remove_file(root.join(WAL)).unwrap();
    let _ = fs::remove_file(root.join(SHM));
    let before = snapshot_tree(root);

    assert!(matches!(
        probe(root, common::lease::lease_port()),
        Err(Error::WalMissing)
    ));
    assert!(matches!(open(root), Err(Error::WalMissing)));
    assert!(matches!(
        Store::recover(root, later(10), common::lease::lease_port()),
        Err(Error::WalMissing)
    ));
    assert!(matches!(read_session(root), Err(Error::WalMissing)));
    assert!(!root.join(WAL).exists());
    assert!(snapshot_tree(root) == before);
}

#[test]
fn probe_leaves_main_and_wal_byte_identical() {
    let seeded = small_store();
    let root = &seeded.root;
    let main = fs::read(root.join(MAIN)).unwrap();
    let wal = fs::read(root.join(WAL)).unwrap();
    assert!(wal.len() > 32);
    for _ in 0..3 {
        assert!(probe(root, common::lease::lease_port()).unwrap().is_some());
        assert!(fs::read(root.join(MAIN)).unwrap() == main);
        assert!(fs::read(root.join(WAL)).unwrap() == wal);
    }
    assert!(!has_snapshot_dir(root));
}

#[test]
fn a_nonempty_journal_refuses_before_sqlite_opens_and_wins_over_a_missing_wal() {
    let seeded = small_store();
    let root = &seeded.root;
    let _ = fs::remove_file(root.join(SHM));
    fs::write(root.join("store.sqlite3-journal"), b"pending").unwrap();
    let before = snapshot_tree(root);
    for missing_wal in [false, true] {
        if missing_wal {
            fs::rename(root.join(WAL), root.join("moved-wal")).unwrap();
        }
        let before = if missing_wal {
            snapshot_tree(root)
        } else {
            before.clone()
        };
        assert!(matches!(
            probe(root, common::lease::lease_port()),
            Err(Error::NeedsCheckpoint)
        ));
        assert!(matches!(open(root), Err(Error::NeedsCheckpoint)));
        assert!(matches!(
            Store::recover(root, later(10), common::lease::lease_port()),
            Err(Error::NeedsCheckpoint)
        ));
        // SQLite never ran: it would have created `-shm`.
        assert!(!root.join(SHM).exists());
        assert!(snapshot_tree(root) == before);
    }
}

#[test]
fn an_empty_journal_is_tolerated_and_a_symlinked_wal_is_wal_missing() {
    let seeded = small_store();
    let root = &seeded.root;
    fs::write(root.join("store.sqlite3-journal"), b"").unwrap();
    assert!(probe(root, common::lease::lease_port()).unwrap().is_some());
    fs::remove_file(root.join("store.sqlite3-journal")).unwrap();

    let target = seeded.scratch.path().join("elsewhere-wal");
    fs::rename(root.join(WAL), &target).unwrap();
    std::os::unix::fs::symlink(&target, root.join(WAL)).unwrap();
    let before = snapshot_tree(root);
    assert!(matches!(
        probe(root, common::lease::lease_port()),
        Err(Error::WalMissing)
    ));
    assert!(matches!(open(root), Err(Error::WalMissing)));
    assert!(matches!(
        Store::recover(root, later(10), common::lease::lease_port()),
        Err(Error::WalMissing)
    ));
    assert!(snapshot_tree(root) == before);
}

#[test]
fn a_symlinked_shm_is_an_invalid_root() {
    let seeded = small_store();
    let root = &seeded.root;
    let target = seeded.scratch.path().join("elsewhere-shm");
    fs::write(&target, b"").unwrap();
    let _ = fs::remove_file(root.join(SHM));
    std::os::unix::fs::symlink(&target, root.join(SHM)).unwrap();
    assert!(matches!(
        probe(root, common::lease::lease_port()),
        Err(Error::InvalidRoot)
    ));
    assert!(matches!(open(root), Err(Error::InvalidRoot)));
}

#[test]
fn no_snapshot_directory_is_ever_created_by_open_probe_or_recover() {
    let seeded = small_store();
    let root = &seeded.root;
    let parent = seeded.scratch.path().to_path_buf();
    let names_before = in_place_support::names(&parent);
    assert!(probe(root, common::lease::lease_port()).unwrap().is_some());
    drop(open(root).unwrap());
    Store::recover(root, later(10), common::lease::lease_port()).unwrap();
    assert!(!has_snapshot_dir(root));
    assert_eq!(in_place_support::names(&parent), names_before);
}

#[test]
fn the_last_owner_close_leaves_a_regular_wal() {
    let seeded = small_store();
    let root = &seeded.root;
    let store = open(root).unwrap();
    store
        .apply_batch(
            &[Mutation::Put {
                key: "k".into(),
                value: b"v".to_vec(),
            }],
            later(10),
        )
        .unwrap();
    assert!(store.checkpoint_passive(later(10)).unwrap().complete);
    drop(store);
    let metadata = fs::symlink_metadata(root.join(WAL)).unwrap();
    assert!(metadata.is_file());
    // A probe's close must not remove it either.
    assert!(probe(root, common::lease::lease_port()).unwrap().is_some());
    assert!(fs::symlink_metadata(root.join(WAL)).unwrap().is_file());
    drop(open(root).unwrap());
    assert!(fs::symlink_metadata(root.join(WAL)).unwrap().is_file());
}
