#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Flush order and failures of the commit watermark: each commit flushes the file as it
//! stands, writes the non-best slot and flushes again. A failure before the transaction
//! commits nothing; a failed flush poisons the handle for writes only.
mod common;
mod watermark_support;
use common::deadline;
use hf_application::watermark::{NAME, Slot};
use hf_storage::{Error, ReadRefusal};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, sync::Arc};
use watermark_support::{
    Recorder, at, database_seq, decoded, file_bytes, file_of, generation, initialized, present,
    put, reopen, session, tear_slot, write_file,
};

#[test]
fn each_commit_flushes_the_old_image_then_the_new_one() {
    let port = Arc::new(Recorder::default());
    let (root, store) = initialized(port.clone());
    let root = &root.root;
    let generation = generation(root);
    let mut expected = vec![file_of(&generation, Some(0), Some(0))];
    let mut current = (Some(0), Some(0));
    for number in 1..=3u64 {
        expected.push(file_of(&generation, current.0, current.1));
        put(&store, &format!("key-{number}")).unwrap();
        current = if number % 2 == 1 {
            (current.0, Some(number))
        } else {
            (Some(number), current.1)
        };
        expected.push(file_of(&generation, current.0, current.1));
        assert_eq!(port.count(), 1 + 2 * usize::try_from(number).unwrap());
    }
    let images: Vec<Vec<u8>> = port
        .images()
        .into_iter()
        .map(|(ok, image)| {
            assert!(ok);
            image
        })
        .collect();
    assert_eq!(
        images, expected,
        "init, then pre-flush and post-flush per commit"
    );
    assert_eq!(file_bytes(root), expected[expected.len() - 1]);
}

#[test]
fn a_failed_pre_flush_commits_nothing_and_a_failed_post_flush_keeps_the_commit() {
    // (flushes allowed before the failure, error, database sequence after, file after)
    let cases = [
        (0, "pre-flush", 0, (Some(0), Some(0))),
        (1, "post-flush", 1, (Some(0), Some(1))),
    ];
    for (allowed, label, seq, slots) in cases {
        let port = Arc::new(Recorder::default());
        let (root, store) = initialized(port.clone());
        let root = &root.root;
        port.fail_after(allowed);
        let result = put(&store, "one");
        let unknown = allowed == 1;
        assert!(
            matches!(
                (&result, unknown),
                (Err(Error::WatermarkUnrecorded), false) | (Err(Error::CommitOutcomeUnknown), true)
            ),
            "{label}: {result:?}"
        );
        assert_eq!(database_seq(root), seq, "{label}");
        assert_eq!(
            file_bytes(root),
            file_of(&generation(root), slots.0, slots.1),
            "{label}"
        );
        // Poisoned: the handle takes no further write even once flushes work again.
        port.heal();
        let flushes = port.count();
        assert!(
            matches!(put(&store, "two"), Err(Error::WatermarkUnrecorded)),
            "{label}"
        );
        assert_eq!(
            port.count(),
            flushes,
            "{label}: a refused write flushes nothing"
        );
        assert_eq!(database_seq(root), seq, "{label}");
        drop(store);
        let store = reopen(root, Arc::new(Recorder::default())).unwrap();
        put(&store, "three").unwrap();
        assert_eq!(database_seq(root), seq + 1, "{label}");
    }
}

#[test]
fn a_poisoned_handle_still_reads_and_checkpoints() {
    let port = Arc::new(Recorder::default());
    let (root, store) = initialized(port.clone());
    let root = &root.root;
    put(&store, "one").unwrap();
    port.fail_after(1);
    assert!(matches!(
        put(&store, "two"),
        Err(Error::CommitOutcomeUnknown)
    ));
    port.heal();
    assert_eq!(present(&store, &["one", "two"]), vec!["one", "two"]);
    let checkpoint = store.checkpoint_passive(deadline()).unwrap();
    assert!(checkpoint.complete);
    assert_eq!(present(&store, &["one", "two"]), vec!["one", "two"]);
    assert!(matches!(
        put(&store, "three"),
        Err(Error::WatermarkUnrecorded)
    ));
    // Another handle on the same root is not poisoned, and a reader sees both commits.
    let other = reopen(root, Arc::new(Recorder::default())).unwrap();
    put(&other, "four").unwrap();
    assert_eq!(present(&store, &["four"]), vec!["four"]);
    session(root).unwrap();
    let refusal = ReadRefusal::from(&Error::WatermarkUnrecorded);
    assert!(matches!(refusal, ReadRefusal::Other { .. }), "{refusal:?}");
}

/// Puts defect `label` in place of the watermark at `path` and returns the error the
/// next commit must report.
fn plant(label: &str, path: &Path, good: &[u8]) -> fn(&Error) -> bool {
    let unavailable: fn(&Error) -> bool =
        |e| matches!(e, Error::ReadUnavailable("store.watermark"));
    let incompatible: fn(&Error) -> bool = |e| matches!(e, Error::Incompatible);
    match label {
        "missing" => fs::remove_file(path).unwrap(),
        "no valid slot" => fs::write(path, vec![0u8; 8192]).unwrap(),
        "short" => fs::write(path, &good[..8191]).unwrap(),
        "long" => fs::write(path, [good, &[0]].concat()).unwrap(),
        "foreign" => fs::write(path, file_of(&"f".repeat(32), Some(1), None)).unwrap(),
        "directory" => {
            fs::remove_file(path).unwrap();
            fs::create_dir(path).unwrap();
        }
        "unreadable" => {
            fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
            return |e| matches!(e, Error::Io(_));
        }
        other => panic!("unknown defect {other}"),
    }
    match label {
        "missing" | "no valid slot" => unavailable,
        _ => incompatible,
    }
}

/// The watermark is opened and checked before `BEGIN`: each defect refuses the commit,
/// commits nothing, writes no slot and leaves the handle usable once it is repaired.
#[test]
fn watermark_defects_before_the_transaction_commit_nothing_and_do_not_poison() {
    let port = Arc::new(Recorder::default());
    let (root, store) = initialized(port.clone());
    let root = &root.root;
    put(&store, "one").unwrap();
    let good = file_bytes(root);
    let path = root.join(NAME);
    let labels = [
        "missing",
        "no valid slot",
        "short",
        "long",
        "foreign",
        "directory",
        "unreadable",
    ];
    for label in labels {
        let want = plant(label, &path, &good);
        let flushes = port.count();
        let result = put(&store, label);
        assert!(result.as_ref().is_err_and(want), "{label}: {result:?}");
        assert_eq!(port.count(), flushes, "{label}: nothing flushed");
        assert_eq!(database_seq(root), 1, "{label}: nothing committed");
        if path.is_dir() {
            fs::remove_dir(&path).unwrap();
        } else if path.exists() {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        fs::write(&path, &good).unwrap();
    }
    put(&store, "repaired").unwrap();
    assert_eq!(decoded(root), at(Slot::A, 2));
}

#[test]
fn a_database_below_the_watermark_refuses_the_commit_without_poisoning() {
    let (root, store) = initialized(Arc::new(Recorder::default()));
    let root = &root.root;
    put(&store, "one").unwrap();
    let good = file_bytes(root);
    let generation = generation(root);
    write_file(root, &file_of(&generation, Some(5), Some(1)));
    assert!(matches!(put(&store, "two"), Err(Error::CommitLost)));
    assert_eq!(database_seq(root), 1);
    assert_eq!(file_bytes(root), file_of(&generation, Some(5), Some(1)));
    write_file(root, &good);
    put(&store, "three").unwrap();
    assert_eq!(
        present(&store, &["one", "two", "three"]),
        vec!["one", "three"]
    );
}

#[test]
fn a_torn_best_slot_after_a_failed_post_flush_still_leaves_the_previous_entry() {
    let port = Arc::new(Recorder::default());
    let (root, store) = initialized(port.clone());
    let root = &root.root;
    put(&store, "one").unwrap();
    port.fail_after(1);
    assert!(matches!(
        put(&store, "two"),
        Err(Error::CommitOutcomeUnknown)
    ));
    drop(store);
    // The unflushed slot A (seq 2) is torn by the power loss: slot B still holds 1.
    tear_slot(root, Slot::A);
    assert_eq!(decoded(root), at(Slot::B, 1));
    session(root).unwrap();
    let store = reopen(root, Arc::new(Recorder::default())).unwrap();
    put(&store, "three").unwrap();
    assert_eq!(decoded(root), at(Slot::A, 3));
}
