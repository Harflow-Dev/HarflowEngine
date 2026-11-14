#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The state-recovery gate (`store.recovery`): a `pending` record refuses every store
//! opener as `RecoveryInterrupted` before any marker, database or sidecar check, at
//! every prefix of the recovery's move order; `committed` and absent pass; a malformed or
//! non-regular record is `Incompatible`; a failure without a pending record keeps its
//! original error.
mod common;
mod read_session_support;
mod recovery_gate_support;
use hf_application::recovery::RecoveryError;
use hf_domain::ctl::state_recovery::{RecoveryState, StoreRecovery};
use hf_storage::Error;
use read_session_support::{names, open, permissions_bind, read_all, seeded};
use recovery_gate_support::{
    OP, OTHER_OP, Outcome, RECORD, assert_all_interrupted, copy_root, move_out, outcomes, r7_order,
    record_bytes, write_record,
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};

#[test]
fn pending_refuses_open_recover_probe_roothold_recovery_read_and_read_session_as_recovery_interrupted()
 {
    let seeded = seeded(3);
    let root = &seeded.root;
    let record = StoreRecovery::new(OP, RecoveryState::Pending).unwrap();
    hf_storage::write_recovery_record(root, &record).unwrap();
    assert_eq!(
        fs::read(root.join(RECORD)).unwrap(),
        record_bytes(OP, "pending")
    );
    let before = common::snapshot_tree(root);
    for (name, outcome) in outcomes(root) {
        assert!(
            matches!(
                outcome,
                Outcome::Store(Error::RecoveryInterrupted)
                    | Outcome::Recovery(RecoveryError::Unavailable)
            ),
            "{name}: {outcome:?}"
        );
        if let Outcome::Store(error) = &outcome {
            assert_eq!(
                hf_storage::ReadRefusal::from(error),
                hf_storage::ReadRefusal::RecoveryInterrupted,
                "{name}"
            );
        }
    }
    assert_eq!(
        common::snapshot_tree(root),
        before,
        "a refusal writes nothing"
    );
}

#[test]
fn pending_refuses_with_store_files_moved_one_by_one() {
    let seeded = seeded(3);
    let template = &seeded.root;
    let order = r7_order(template);
    for name in ["store.sqlite3", "store.sqlite3-wal", "store.watermark"] {
        assert!(
            order.iter().any(|entry| entry == name),
            "{name} in {order:?}"
        );
    }
    for moved in 0..=order.len() {
        let parent = seeded.scratch.path().join(format!("prefix-{moved}"));
        fs::create_dir(&parent).unwrap();
        let root = copy_root(template, &parent, "state");
        write_record(&root, "pending");
        for name in &order[..moved] {
            move_out(&root, &parent.join("data"), name);
        }
        let listed = names(&root);
        let label = format!("moved {:?}", &order[..moved]);
        for (name, outcome) in outcomes(&root) {
            assert!(
                matches!(
                    outcome,
                    Outcome::Store(Error::RecoveryInterrupted)
                        | Outcome::Recovery(RecoveryError::Unavailable)
                ),
                "{label}: {name}: {outcome:?}"
            );
        }
        assert_eq!(names(&root), listed, "{label}: nothing created in the root");
    }
}

#[test]
fn pre_lease_failure_without_pending_keeps_original_error() {
    let seeded = seeded(3);
    let template = &seeded.root;
    for (index, missing) in ["store.identity", "store.sqlite3", "store.lock", "io.lock"]
        .into_iter()
        .enumerate()
    {
        let parent = seeded.scratch.path().join(format!("missing-{index}"));
        fs::create_dir(&parent).unwrap();
        let root = copy_root(template, &parent, "state");
        fs::remove_file(root.join(missing)).unwrap();
        let baseline = outcomes(&root);
        for (name, outcome) in &baseline {
            assert!(
                matches!(
                    outcome,
                    Outcome::Store(Error::Io(_) | Error::InvalidRoot | Error::Incompatible)
                        | Outcome::Store(Error::ReadUnavailable(_))
                        | Outcome::Recovery(RecoveryError::Invalid | RecoveryError::Unavailable)
                ),
                "{missing}: {name}: {outcome:?}"
            );
            assert!(
                !matches!(outcome, Outcome::Store(Error::RecoveryInterrupted)),
                "{missing}: {name}"
            );
        }
        for (label, state) in [
            ("committed", Some(record_bytes(OP, "committed"))),
            (
                "committed for another operation",
                Some(record_bytes(OTHER_OP, "committed")),
            ),
            ("absent", None),
        ] {
            match &state {
                Some(bytes) => fs::write(root.join(RECORD), bytes).unwrap(),
                None => drop(fs::remove_file(root.join(RECORD))),
            }
            for ((name, want), (_, got)) in baseline.iter().zip(outcomes(&root)) {
                assert_eq!(got.kind(), want.kind(), "{missing}, {label}: {name}");
            }
        }
    }
}

/// Gate point 1 leaves an I/O failure reading the record to identification, so a root
/// that is a regular file or is missing keeps its error.
#[test]
fn a_root_that_is_not_a_directory_keeps_its_error() {
    let fixture = common::Fixture::new();
    let file = fixture.path().join("state");
    fs::write(&file, b"not a directory").unwrap();
    for (name, outcome) in outcomes(&file) {
        assert!(
            matches!(
                outcome,
                Outcome::Store(Error::InvalidRoot | Error::Io(_))
                    | Outcome::Recovery(RecoveryError::Invalid)
            ),
            "regular file: {name}: {outcome:?}"
        );
    }
    for (name, outcome) in outcomes(&fixture.path().join("absent")) {
        assert!(
            !matches!(outcome, Outcome::Store(Error::RecoveryInterrupted)),
            "missing: {name}: {outcome:?}"
        );
    }
}

#[test]
fn committed_record_passes() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let baseline: Vec<String> = outcomes(root).iter().map(|(_, o)| o.kind()).collect();
    for operation in [OP, OTHER_OP] {
        let record = StoreRecovery::new(operation, RecoveryState::Committed).unwrap();
        hf_storage::write_recovery_record(root, &record).unwrap();
        let bytes = fs::read(root.join(RECORD)).unwrap();
        assert_eq!(bytes, record_bytes(operation, "committed"));
        for ((name, outcome), want) in outcomes(root).iter().zip(&baseline) {
            assert_eq!(&outcome.kind(), want, "{operation}: {name}");
        }
        for (name, outcome) in outcomes(root) {
            if name != "recovery read" {
                assert!(matches!(outcome, Outcome::Opened), "{name}: {outcome:?}");
            }
        }
        assert_eq!(read_all(&open(root).unwrap()).unwrap().len(), 3);
        assert_eq!(
            fs::read(root.join(RECORD)).unwrap(),
            bytes,
            "record untouched"
        );
    }
}

#[test]
fn record_malformed_or_non_regular_is_incompatible() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let path = root.join(RECORD);
    let elsewhere = seeded.scratch.path().join("pending-elsewhere");
    fs::write(&elsewhere, record_bytes(OP, "pending")).unwrap();
    let malformed: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("garbage", b"\x00garbage".to_vec()),
        ("no final newline", {
            let mut bytes = record_bytes(OP, "pending");
            bytes.pop();
            bytes
        }),
        ("unknown state", record_bytes(OP, "done")),
        (
            "uppercase operation",
            record_bytes(&OP.to_uppercase(), "pending"),
        ),
        ("short operation", record_bytes(&OP[..16], "pending")),
        (
            "other header",
            b"harflow-recovery-2\n0123456789abcdef0123456789abcdef\npending\n".to_vec(),
        ),
        (
            "oversized",
            [record_bytes(OP, "pending"), vec![b'x'; 128]].concat(),
        ),
    ];
    for (label, bytes) in malformed {
        fs::write(&path, &bytes).unwrap();
        assert_incompatible(root, label);
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert_incompatible(root, "directory");
    fs::remove_dir(&path).unwrap();
    symlink(&elsewhere, &path).unwrap();
    assert_incompatible(root, "symlink to a pending record");
    fs::remove_file(&path).unwrap();
    assert_eq!(
        read_all(&open(root).unwrap()).unwrap().len(),
        3,
        "absent passes"
    );
}

fn assert_incompatible(root: &Path, label: &str) {
    for (name, outcome) in outcomes(root) {
        assert!(
            matches!(
                outcome,
                Outcome::Store(Error::Incompatible) | Outcome::Recovery(RecoveryError::Invalid)
            ),
            "{label}: {name}: {outcome:?}"
        );
    }
}

/// An unreadable record passes the lease-free read (an I/O failure there is left to
/// identification) and is reported by the read under the lease.
#[test]
fn an_unreadable_record_is_reported_under_the_lease() {
    let seeded = seeded(3);
    let root = &seeded.root;
    if !permissions_bind(seeded.scratch.path()) {
        return;
    }
    let path = root.join(RECORD);
    fs::write(&path, record_bytes(OP, "pending")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    for (name, outcome) in outcomes(root) {
        let expected = match name {
            "read session" => matches!(outcome, Outcome::Store(Error::ReadUnavailable(RECORD))),
            "recovery read" => matches!(outcome, Outcome::Recovery(RecoveryError::Unavailable)),
            _ => matches!(&outcome, Outcome::Store(Error::Io(error))
                if error.kind() == std::io::ErrorKind::PermissionDenied),
        };
        assert!(expected, "{name}: {outcome:?}");
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
}

/// The lease-free gate reads nothing through a path with a symlinked ancestor, so a
/// pending record behind one is refused as the path (`InvalidRoot`) by every opener, as
/// without the record; the same record at the real path refuses every opener.
#[test]
fn pending_behind_a_symlinked_ancestor_is_refused_as_the_path_by_every_opener() {
    let fixture = common::Fixture::new();
    let real = fixture.path().join("real");
    fs::create_dir(&real).unwrap();
    let seeded = seeded(3);
    copy_root(&seeded.root, &real, "state");
    let link = fixture.path().join("link");
    symlink(&real, &link).unwrap();
    let root = link.join("state");
    for (name, outcome) in outcomes(&root) {
        assert!(
            matches!(
                outcome,
                Outcome::Store(Error::InvalidRoot) | Outcome::Recovery(RecoveryError::Invalid)
            ),
            "no record: {name}: {outcome:?}"
        );
    }
    write_record(&real.join("state"), "pending");
    // The gate reads nothing through a symlinked path: every opener rejects the path.
    for (name, outcome) in outcomes(&root) {
        assert!(
            matches!(
                outcome,
                Outcome::Store(Error::InvalidRoot) | Outcome::Recovery(RecoveryError::Invalid)
            ),
            "pending: {name}: {outcome:?}"
        );
    }
    assert_all_interrupted(&real.join("state"), "pending at the real path");
}

/// An unreadable pending record and a moved marker or database: the post-failure re-read
/// cannot read the record, so every storage opener reports the record's I/O failure, not
/// the moved file's `Incompatible` or `InvalidRoot`.
#[test]
fn an_unreadable_pending_record_with_a_moved_file_reports_the_record_failure() {
    let seeded = seeded(3);
    if !permissions_bind(seeded.scratch.path()) {
        return;
    }
    for (index, moved) in ["store.identity", "store.sqlite3"].into_iter().enumerate() {
        let parent = seeded.scratch.path().join(format!("unreadable-{index}"));
        fs::create_dir(&parent).unwrap();
        let root = copy_root(&seeded.root, &parent, "state");
        let path = root.join(RECORD);
        write_record(&root, "pending");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        move_out(&root, &parent.join("data"), moved);
        for (name, outcome) in outcomes(&root) {
            let expected = match name {
                "recovery read" => {
                    matches!(outcome, Outcome::Recovery(RecoveryError::Unavailable))
                }
                _ => matches!(&outcome, Outcome::Store(Error::Io(error))
                    if error.kind() == std::io::ErrorKind::PermissionDenied),
            };
            assert!(expected, "{moved}: {name}: {outcome:?}");
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}
