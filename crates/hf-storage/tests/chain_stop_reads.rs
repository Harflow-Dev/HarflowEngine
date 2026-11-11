#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The record read with the stop row of its chain root: one statement, one snapshot, the join
//! on the stored chain root or else on the task's own id.
mod common;
mod fixture_support;
use fixture_support::{A, B, C, World, done, later, tid, world};
use hf_domain::ctl::{continuation_stop::StopRow, task_rows::ChainRoot};
use hf_storage::{ChainStop, statements};

const MALFORMED: &str = "not a valid id!";

fn put(w: &World, root: &str, stop: &ChainStop) {
    w.store.put_chain_stop(&tid(root), stop, later(5)).unwrap();
}

fn marker(text: Option<&str>) -> ChainStop {
    ChainStop::Marker {
        cancelled_task_id: text.map(str::to_owned),
    }
}

/// A task `id` whose stored chain root is `root` (raw text, valid or not).
fn member(w: &World, id: &str, root: Option<&str>) {
    let mut task = done(id);
    task.facts.chain_root = root.map(str::to_owned);
    w.add(&task);
}

fn read(w: &World, id: &str) -> Option<(ChainRoot, StopRow)> {
    w.session()
        .snapshot(later(30), |snapshot| snapshot.record_with_stop(&tid(id)))
        .unwrap()
        .map(|(record, row)| (record.facts.chain_root, row))
}

fn row(w: &World, id: &str) -> StopRow {
    read(w, id).unwrap().1
}

#[test]
fn an_unknown_task_has_no_record() {
    let w = world();
    assert!(read(&w, A).is_none());
}

#[test]
fn no_row_for_the_root_is_absent() {
    let w = world();
    member(&w, A, None);
    assert_eq!(row(&w, A), StopRow::Absent);
}

#[test]
fn a_marker_with_a_valid_attribution_keeps_it() {
    let w = world();
    member(&w, A, None);
    put(&w, A, &marker(Some(B)));
    assert_eq!(
        row(&w, A),
        StopRow::Marker {
            attribution: Some(B.to_owned())
        }
    );
}

#[test]
fn a_marker_without_attribution_has_none() {
    let w = world();
    member(&w, A, None);
    put(&w, A, &marker(None));
    assert_eq!(row(&w, A), StopRow::Marker { attribution: None });
}

#[test]
fn malformed_text_up_to_the_limit_is_copied_for_the_domain_to_judge() {
    let w = world();
    member(&w, A, None);
    put(&w, A, &marker(Some(MALFORMED)));
    assert_eq!(
        row(&w, A),
        StopRow::Marker {
            attribution: Some(MALFORMED.to_owned())
        }
    );
}

#[test]
fn attribution_length_boundaries_are_one_to_sixty_four_bytes() {
    let w = world();
    for (id, text, expected) in [
        (A, "x".repeat(64), Some("x".repeat(64))),
        (B, "x".repeat(65), None),
        (C, String::new(), None),
    ] {
        member(&w, id, None);
        put(&w, id, &marker(Some(&text)));
        assert_eq!(
            row(&w, id),
            StopRow::Marker {
                attribution: expected
            },
            "{} bytes",
            text.len()
        );
    }
}

#[test]
fn an_oversize_attribution_is_never_copied() {
    let w = world();
    member(&w, A, None);
    let text = format!("{B}{}", "x".repeat(4096));
    put(&w, A, &marker(Some(&text)));
    assert_eq!(row(&w, A), StopRow::Marker { attribution: None });
}

#[test]
fn the_length_limit_counts_bytes_not_characters() {
    let w = world();
    member(&w, A, None);
    // 33 two-byte characters: 33 characters but 66 bytes.
    put(&w, A, &marker(Some(&"é".repeat(33))));
    assert_eq!(row(&w, A), StopRow::Marker { attribution: None });
}

#[test]
fn a_non_utf8_attribution_reads_as_none_and_never_fails() {
    let w = world();
    member(&w, A, None);
    member(&w, B, Some(MALFORMED));
    w.raw()
        .execute_batch(&format!(
            "INSERT INTO chain_stops VALUES('{A}','marker',CAST(x'80' AS TEXT));\
             INSERT INTO chain_stops VALUES('{MALFORMED}','marker',CAST(x'80' AS TEXT));"
        ))
        .unwrap();
    assert_eq!(row(&w, A), StopRow::Marker { attribution: None });
    // The invalid root's row is read without error; the domain ignores it.
    assert!(read(&w, B).is_some());
}

#[test]
fn a_fault_row_reads_as_a_fault() {
    let w = world();
    member(&w, A, None);
    put(&w, A, &ChainStop::Fault);
    assert_eq!(row(&w, A), StopRow::Fault);
}

#[test]
fn a_member_joins_on_its_stored_chain_root() {
    let w = world();
    member(&w, A, None);
    member(&w, B, Some(A));
    put(&w, A, &marker(Some(C)));
    let (root, stop) = read(&w, B).unwrap();
    assert_eq!(root, ChainRoot::Valid(tid(A)));
    assert_eq!(
        stop,
        StopRow::Marker {
            attribution: Some(C.to_owned())
        }
    );
}

#[test]
fn a_stored_root_is_used_instead_of_the_task_id() {
    let w = world();
    member(&w, A, None);
    member(&w, B, Some(A));
    // A row for the member's own id does not apply while a root is stored.
    put(&w, B, &ChainStop::Fault);
    assert_eq!(row(&w, B), StopRow::Absent);
    assert_eq!(row(&w, A), StopRow::Absent);
}

#[test]
fn without_a_stored_root_the_task_id_is_the_key() {
    let w = world();
    member(&w, B, None);
    put(&w, B, &marker(None));
    put(&w, A, &ChainStop::Fault);
    assert_eq!(row(&w, B), StopRow::Marker { attribution: None });
}

#[test]
fn an_empty_stored_root_matches_no_row_and_reads_as_invalid() {
    let w = world();
    member(&w, A, Some(""));
    put(&w, A, &ChainStop::Fault);
    let (root, stop) = read(&w, A).unwrap();
    assert_eq!(root, ChainRoot::Invalid(String::new()));
    assert_eq!(stop, StopRow::Absent);
}

#[test]
fn an_invalid_stored_root_equal_to_a_table_key_is_still_invalid() {
    let w = world();
    // The writer takes identities only, but the table accepts any 1..=64 character root, so a
    // row keyed by the malformed text can exist. The record's root must stay invalid; the
    // domain, not the join, decides that the stop state is unknown.
    w.raw()
        .execute(
            "INSERT INTO chain_stops VALUES(?1,'fault',NULL)",
            [MALFORMED],
        )
        .unwrap();
    member(&w, A, Some(MALFORMED));
    let (root, stop) = read(&w, A).unwrap();
    assert_eq!(root, ChainRoot::Invalid(MALFORMED.to_owned()));
    assert_eq!(stop, StopRow::Fault);
}

#[test]
fn the_statement_searches_the_stop_table_by_its_key() {
    let w = world();
    member(&w, A, None);
    let db = w.db();
    let mut statement = db
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            statements::record_with_stop()
        ))
        .unwrap();
    let plan = statement
        .query_map(rusqlite::params![A], |r| r.get::<_, String>(3))
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        plan.contains("SEARCH c USING")
            && (plan.contains("PRIMARY KEY") || plan.contains("autoindex")),
        "{plan}"
    );
    assert!(!plan.contains("SCAN c"), "{plan}");
}
