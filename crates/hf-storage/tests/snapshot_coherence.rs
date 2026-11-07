#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! One snapshot is one coherent state, even while another connection writes between two of
//! its reads, and a row the typed mapping cannot accept is a `MalformedRow` refusal.
mod common;
mod fixture_support;
use common::{deadline, default_limits};
use fixture_support::{A, B, MAIN, done, task, tid, world};
use hf_domain::ctl::task_rows::BatchQuery;
use hf_storage::{Error, ReadSession};
use rusqlite::Connection;

fn session(root: &std::path::Path) -> ReadSession {
    ReadSession::open(
        root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap()
}

fn ids(snapshot: &hf_storage::TaskSnapshot<'_>) -> Vec<String> {
    snapshot
        .batch(&BatchQuery {
            max_rows: 100,
            ..BatchQuery::default()
        })
        .unwrap()
        .iter()
        .map(|record| record.facts.id.as_str().to_string())
        .collect()
}

#[test]
fn a_write_between_two_reads_of_one_snapshot_is_not_seen_by_either() {
    let w = world();
    w.add(&done(A));
    w.add(&task(B, "running", None, false));
    let (root, store) = (w.root.clone(), w.store);
    drop(store);
    let writer = Connection::open(root.join(MAIN)).unwrap();
    let reader = session(&root);
    let (first, second) = reader
        .snapshot(deadline(), |snapshot| {
            let first = (
                snapshot.damaged()?,
                snapshot.unobserved_count()?,
                ids(snapshot),
            );
            // Another connection removes one task between the reads.
            writer
                .execute_batch(
                    "DELETE FROM task_headers WHERE id='bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb';\
                     DELETE FROM task_facts WHERE id='bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb';",
                )
                .unwrap();
            let second = (
                snapshot.damaged()?,
                snapshot.unobserved_count()?,
                ids(snapshot),
            );
            Ok((first, second))
        })
        .unwrap();
    assert_eq!(first, (false, 1, vec![B.to_string(), A.to_string()]));
    assert_eq!(second, first, "one snapshot is one state");
    let later = reader
        .snapshot(deadline(), |snapshot| {
            Ok((snapshot.damaged()?, snapshot.unobserved_count()?))
        })
        .unwrap();
    assert_eq!(later, (false, 0), "the next snapshot sees the write");
    let _ = tid(A);
}

#[test]
fn a_row_the_typed_mapping_cannot_accept_is_malformed() {
    let w = world();
    w.add(&done(A));
    let (root, store) = (w.root.clone(), w.store);
    drop(store);
    let writer = Connection::open(root.join(MAIN)).unwrap();
    writer
        .execute_batch(
            "UPDATE task_facts SET id='bad id' WHERE id='aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa';\
             UPDATE task_headers SET id='bad id' WHERE id='aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa';",
        )
        .unwrap();
    let reader = session(&root);
    let outcome = reader.snapshot(deadline(), |snapshot| {
        snapshot.batch(&BatchQuery {
            max_rows: 100,
            ..BatchQuery::default()
        })
    });
    assert!(matches!(outcome, Err(Error::MalformedRow)), "{outcome:?}");
}
