#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! A whole-store check that finds corruption fences the generation: every later
//! commit, writer open, probe, recover and read refuses. A fence that does not name
//! the current generation, or is not a well-formed regular file, is `Incompatible`.
mod common;
mod read_session_support;
mod verify_support;
use hf_storage::{Error, Mutation, ReadRefusal, Store, Verification};
use read_session_support::{later, open, read_all, seeded};
use std::{fs, os::unix::fs::symlink};
use verify_support::{
    FENCE, VERIFY, assert_entries_refuse, at, clear, damaged, fence_bytes, generation, is_fenced,
    is_incompatible, open_store, verification_files,
};

fn put(store: &Store, key: &str) -> hf_storage::Result<()> {
    store.apply_batch(
        &[Mutation::Put {
            key: key.into(),
            value: b"v".to_vec(),
        }],
        later(5),
    )
}

/// A labelled way to occupy a file name.
type Plant<'a> = (&'static str, Box<dyn Fn() + 'a>);

#[test]
fn corruption_fences_and_every_entry_then_refuses() {
    let seeded = damaged();
    let root = &seeded.root;
    let session = open(root).unwrap();
    assert_eq!(read_all(&session).unwrap().len(), 3, "baseline read");
    let store = open_store(root).unwrap();
    put(&store, "before").unwrap();

    assert_eq!(
        store.verify_in_place(later(30)).unwrap(),
        Verification::Fenced
    );
    assert_eq!(
        fs::read(root.join(FENCE)).unwrap(),
        fence_bytes(&generation(root), "failed")
    );
    assert_eq!(verification_files(root), vec![FENCE.to_string()]);

    // The open handle takes no further commit, and it is not retried into success.
    for attempt in 0..2 {
        assert!(
            matches!(put(&store, "after"), Err(Error::Fenced)),
            "{attempt}"
        );
    }
    assert!(matches!(
        store.verify_in_place(later(30)),
        Err(Error::Fenced)
    ));
    assert!(matches!(
        store.verify_if_due(at(0), later(30)),
        Err(Error::Fenced)
    ));
    // A session opened before the fence refuses its next read.
    let refused = read_all(&session).unwrap_err();
    assert!(is_fenced(&refused), "{refused:?}");
    assert_eq!(ReadRefusal::from(&refused), ReadRefusal::Fenced);
    drop(session);
    drop(store);

    assert_entries_refuse(root, is_fenced, &ReadRefusal::Fenced, "fenced");
    // Nothing was committed after the fence, and refusing wrote nothing more.
    let connection = rusqlite::Connection::open(root.join("store.sqlite3")).unwrap();
    let after: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM fixture_records WHERE key = 'after'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(after, 0);
    assert_eq!(verification_files(root), vec![FENCE.to_string()]);
}

#[test]
fn a_fence_planted_under_an_open_writer_refuses_its_next_commit() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let store = open_store(root).unwrap();
    put(&store, "before").unwrap();
    fs::write(root.join(FENCE), fence_bytes(&generation(root), "failed")).unwrap();
    assert!(matches!(put(&store, "after"), Err(Error::Fenced)));
    // Clearing belongs to maintenance; once it is gone the same handle commits again.
    clear(root, FENCE);
    put(&store, "cleared").unwrap();
}

#[test]
fn a_foreign_generation_fence_is_incompatible_everywhere() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let session = open(root).unwrap();
    let store = open_store(root).unwrap();
    let other = "f".repeat(32);
    assert_ne!(other, generation(root));
    fs::write(root.join(FENCE), fence_bytes(&other, "failed")).unwrap();

    assert!(matches!(put(&store, "k"), Err(Error::Incompatible)));
    assert!(matches!(
        store.verify_in_place(later(30)),
        Err(Error::Incompatible)
    ));
    let refused = read_all(&session).unwrap_err();
    assert!(is_incompatible(&refused), "{refused:?}");
    assert_eq!(ReadRefusal::from(&refused), ReadRefusal::InvalidMarker);
    drop((session, store));
    assert_entries_refuse(
        root,
        is_incompatible,
        &ReadRefusal::InvalidMarker,
        "foreign",
    );
    assert!(!root.join(VERIFY).exists());
}

fn padded(text: &str, fill: u8, length: usize) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.resize(length, fill);
    bytes
}

#[test]
fn malformed_or_oversized_fences_are_incompatible() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let generation = generation(root);
    let valid = String::from_utf8(fence_bytes(&generation, "failed")).unwrap();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("wrong header", valid.replace("-1\n", "-2\n").into_bytes()),
        ("no trailing newline", valid.trim_end().as_bytes().to_vec()),
        ("extra line", format!("{valid}extra\n").into_bytes()),
        ("unknown state", fence_bytes(&generation, "passed")),
        ("upper case state", fence_bytes(&generation, "FAILED")),
        ("crlf", valid.replace('\n', "\r\n").into_bytes()),
        ("128 bytes", padded(&valid, b'x', 128)),
        ("129 bytes, valid prefix", padded(&valid, b'\n', 129)),
        (
            "large",
            format!("{valid}{}", "x".repeat(1 << 20)).into_bytes(),
        ),
    ];
    for (label, bytes) in cases {
        fs::write(root.join(FENCE), &bytes).unwrap();
        assert_entries_refuse(root, is_incompatible, &ReadRefusal::InvalidMarker, label);
    }
    clear(root, FENCE);
    drop(open_store(root).unwrap());
}

#[test]
fn a_non_regular_fence_is_incompatible_and_never_followed() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let elsewhere = seeded.scratch.path().join("elsewhere-fence");
    fs::write(&elsewhere, fence_bytes(&generation(root), "failed")).unwrap();
    let plants: [Plant<'_>; 3] = [
        (
            "directory",
            Box::new(|| fs::create_dir(root.join(FENCE)).unwrap()),
        ),
        (
            "symlink to a valid fence",
            Box::new(|| symlink(&elsewhere, root.join(FENCE)).unwrap()),
        ),
        (
            "dangling symlink",
            Box::new(|| symlink(root.join("missing"), root.join(FENCE)).unwrap()),
        ),
    ];
    for (label, plant) in plants {
        plant();
        // A followed link would read as `Fenced`; the link itself is refused instead.
        assert_entries_refuse(root, is_incompatible, &ReadRefusal::InvalidMarker, label);
        clear(root, FENCE);
    }
    drop(open_store(root).unwrap());
}

#[test]
fn neither_file_exists_after_initialize_open_or_a_due_query() {
    let seeded = seeded(3);
    let root = &seeded.root;
    assert!(verification_files(root).is_empty());
    let store = open_store(root).unwrap();
    assert!(store.verification_due(at(1_000)).unwrap());
    put(&store, "k").unwrap();
    drop(store);
    drop(open(root).unwrap());
    assert!(verification_files(root).is_empty());
}
