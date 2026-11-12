#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::{Fixture, deadline, default_limits};
use hf_storage::{Error, Mutation, SCHEMA_VERSION, Store, probe};
use rusqlite::{Connection, OpenFlags};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::thread;

fn snapshot_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries = fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let path = entry.path();
            (path.clone(), fs::read(path).unwrap())
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
}

#[test]
fn missing_probe_does_not_create_parent_or_root() {
    let fixture = Fixture::new();
    let absent_parent = fixture.path().join("absent");
    let root = absent_parent.join("store");

    assert_eq!(probe(&root, common::lease::lease_port()).unwrap(), None);
    assert!(!absent_parent.exists());
    assert!(!root.exists());
}

#[test]
fn probe_rejects_dangling_root_symlink_and_symlink_ancestor() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let dangling = fixture.path().join("dangling-store");
    symlink(fixture.path().join("does-not-exist"), &dangling).unwrap();
    assert!(matches!(
        probe(&dangling, common::lease::lease_port()),
        Err(Error::InvalidRoot)
    ));

    let ancestor_link = fixture.path().join("linked-parent");
    symlink(fixture.path(), &ancestor_link).unwrap();
    let missing_below_link = ancestor_link.join("not-created").join("store");
    assert!(matches!(
        probe(&missing_below_link, common::lease::lease_port()),
        Err(Error::InvalidRoot)
    ));
    assert!(!fixture.path().join("not-created").exists());
}

#[test]
fn initialization_is_exclusive_and_concurrent_initializers_share_one_generation() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let barrier = Arc::new(Barrier::new(3));
    let workers = (0..2)
        .map(|_| {
            let root = root.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                Store::initialize_fixture(
                    &root,
                    default_limits(),
                    deadline(),
                    common::lease::lease_port(),
                )
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();

    let outcomes = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(
                result,
                Err(Error::InvalidRoot | Error::Incompatible | Error::Busy)
            ))
            .count(),
        1
    );
    assert!(root.join("store.sqlite3").is_file());
}

#[test]
fn foreign_and_empty_roots_refuse_without_changing_any_bytes() {
    for contents in [Some(b"foreign state".as_slice()), None] {
        let fixture = Fixture::new();
        let root = fixture.path().join("store");
        fs::create_dir(&root).unwrap();
        if let Some(bytes) = contents {
            fs::write(root.join("python-state.json"), bytes).unwrap();
        }
        let before = snapshot_tree(&root);

        assert!(matches!(
            Store::initialize_fixture(
                &root,
                default_limits(),
                deadline(),
                common::lease::lease_port()
            ),
            Err(Error::InvalidRoot | Error::Incompatible)
        ));
        assert_eq!(snapshot_tree(&root), before);
    }
}

#[test]
fn future_schema_probe_and_open_leave_database_and_sidecars_untouched() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    drop(
        Store::initialize_fixture(
            &root,
            default_limits(),
            deadline(),
            common::lease::lease_port(),
        )
        .unwrap(),
    );
    let database = root.join("store.sqlite3");
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch("PRAGMA journal_mode=DELETE")
        .unwrap();
    connection
        .pragma_update(None, "application_id", 1212560210_i64)
        .unwrap();
    connection
        .pragma_update(None, "user_version", 900_u32)
        .unwrap();
    drop(connection);
    fs::write(root.join("store.sqlite3-wal"), b"fixture wal marker").unwrap();
    fs::write(root.join("store.sqlite3-shm"), b"fixture shm marker").unwrap();
    let before = snapshot_tree(&root);

    assert!(matches!(
        probe(&root, common::lease::lease_port()),
        Err(Error::FutureSchema)
    ));
    assert!(matches!(
        Store::open(
            &root,
            default_limits(),
            deadline(),
            common::lease::lease_port()
        ),
        Err(Error::FutureSchema)
    ));
    assert_eq!(snapshot_tree(&root), before);
}

#[test]
fn corrupt_database_is_typed_and_left_unchanged() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    drop(
        Store::initialize_fixture(
            &root,
            default_limits(),
            deadline(),
            common::lease::lease_port(),
        )
        .unwrap(),
    );
    fs::write(root.join("store.sqlite3"), b"corrupt database bytes").unwrap();
    let before = snapshot_tree(&root);

    assert!(matches!(
        probe(&root, common::lease::lease_port()),
        Err(Error::Incompatible)
    ));
    assert!(matches!(
        Store::open(
            &root,
            default_limits(),
            deadline(),
            common::lease::lease_port()
        ),
        Err(Error::Incompatible)
    ));
    assert_eq!(snapshot_tree(&root), before);
}

#[cfg(unix)]
#[test]
fn permission_denial_remains_a_typed_io_error_when_host_enforces_it() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    drop(
        Store::initialize_fixture(
            &root,
            default_limits(),
            deadline(),
            common::lease::lease_port(),
        )
        .unwrap(),
    );
    fs::set_permissions(&root, fs::Permissions::from_mode(0o000)).unwrap();
    let result = probe(&root, common::lease::lease_port());
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();

    match result {
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::PermissionDenied => {}
        Ok(Some(_)) => assert_eq!(effective_uid(), 0, "only root may bypass mode 000"),
        unexpected => panic!("unexpected permission probe result: {unexpected:?}"),
    }
}

fn effective_uid() -> u32 {
    let output = std::process::Command::new("id").arg("-u").output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

#[test]
fn live_store_allows_shared_open_and_probe_without_creating_files() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    let before = snapshot_tree(&root);

    assert!(probe(&root, common::lease::lease_port()).unwrap().is_some());
    let second = Store::open(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    assert_eq!(second.identity().generation, store.identity().generation);
    assert_eq!(snapshot_tree(&root), before);
    drop(store);
}

#[test]
fn normal_open_recovers_committed_wal_without_probe_side_effects() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    drop(
        Store::initialize_fixture(
            &root,
            default_limits(),
            deadline(),
            common::lease::lease_port(),
        )
        .unwrap(),
    );
    let connection = common::raw_connection(root.join("store.sqlite3"));
    connection
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    connection
        .execute(
            "INSERT INTO fixture_records VALUES('crash-commit', X'6B657074')",
            [],
        )
        .unwrap();
    let before = common::durable_tree(&root);

    assert!(probe(&root, common::lease::lease_port()).unwrap().is_some());
    assert_eq!(common::durable_tree(&root), before);
    drop(connection);
    let reopened = Store::open(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    assert_eq!(
        reopened
            .read_batch(&["crash-commit".into()], deadline())
            .unwrap()[0]
            .value,
        b"kept"
    );
}

#[cfg(unix)]
#[test]
fn symlink_and_replaced_roots_fail_closed_before_mutation() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let real_root = fixture.path().join("real-store");
    let alias = fixture.path().join("alias-store");
    let store = Store::initialize_fixture(
        &real_root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    symlink(&real_root, &alias).unwrap();
    assert!(matches!(
        Store::open(
            &alias,
            default_limits(),
            deadline(),
            common::lease::lease_port()
        ),
        Err(Error::InvalidRoot)
    ));

    let moved_root = fixture.path().join("moved-store");
    fs::rename(&real_root, &moved_root).unwrap();
    fs::create_dir(&real_root).unwrap();
    fs::write(real_root.join("replacement-marker"), b"preserve").unwrap();
    let before = snapshot_tree(&real_root);

    assert!(matches!(
        store.apply_batch(
            &[Mutation::Insert {
                key: "rejected".into(),
                value: b"no effect".to_vec()
            }],
            deadline(),
        ),
        Err(Error::Replaced)
    ));
    assert_eq!(snapshot_tree(&real_root), before);
    let moved_db = Connection::open(moved_root.join("store.sqlite3")).unwrap();
    let count: u32 = moved_db
        .query_row(
            "SELECT COUNT(*) FROM fixture_records WHERE key='rejected'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn generation_survives_reopen_and_database_uses_wal_mode() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    let store = Store::initialize_fixture(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    let generation = store.identity().generation.clone();
    drop(store);
    let reopened = Store::open(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();

    assert_eq!(reopened.identity().generation, generation);
    assert_eq!(reopened.identity().schema_version, SCHEMA_VERSION);

    let connection = Connection::open_with_flags(
        root.join("store.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
            .unwrap()
            .to_lowercase(),
        "wal"
    );
    drop(connection);
    drop(reopened);
}
