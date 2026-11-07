#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Query plans of the ctl task reads, taken from the statements the snapshot itself runs
//! (`hf_storage::statements`), so a changed predicate or a dropped index fails here.
mod common;
mod fixture_support;
use fixture_support::{A, B, done, task, tid, world};
use hf_domain::ctl::task_rows::BatchQuery;
use hf_storage::statements;
use rusqlite::types::Value;

fn plan(sql: &str) -> String {
    let w = world();
    w.add(&done(A));
    w.add(&task(B, "running", None, false));
    let db = w.db();
    let mut statement = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let marks = statement.parameter_count();
    let rows = statement
        .query_map(
            rusqlite::params_from_iter(vec![Value::Integer(1); marks]),
            |row| row.get::<_, String>(3),
        )
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    rows.join(" | ")
}

fn walk(query: BatchQuery) -> String {
    plan(&statements::batch(&query).0)
}

fn query() -> BatchQuery {
    BatchQuery {
        max_rows: 100,
        ..BatchQuery::default()
    }
}

#[test]
fn the_active_only_walk_uses_the_partial_unobserved_index_without_a_sort() {
    for after in [None, Some((5, "x".to_string()))] {
        let found = walk(BatchQuery {
            unobserved_only: true,
            after,
            ..query()
        });
        assert!(found.contains("facts_unobserved"), "{found}");
        assert!(!found.contains("TEMP B-TREE"), "{found}");
    }
}

#[test]
fn the_default_walk_follows_the_start_order_without_a_sort() {
    for after in [None, Some((5, "x".to_string()))] {
        let found = walk(BatchQuery { after, ..query() });
        assert!(found.contains("facts_started"), "{found}");
        assert!(!found.contains("TEMP B-TREE"), "{found}");
    }
    let found = walk(BatchQuery {
        since_us: Some(5),
        ..query()
    });
    assert!(found.contains("facts_started"), "{found}");
}

#[test]
fn the_session_walk_uses_the_session_index_without_a_sort() {
    for after in [None, Some((5, "x".to_string()))] {
        let found = walk(BatchQuery {
            session: Some("s".into()),
            after,
            ..query()
        });
        assert!(found.contains("facts_session"), "{found}");
        assert!(!found.contains("TEMP B-TREE"), "{found}");
    }
}

#[test]
fn the_predecessor_walk_uses_the_continuation_index() {
    let found = walk(BatchQuery {
        predecessor: Some(tid(A)),
        ..query()
    });
    assert!(found.contains("facts_continuation"), "{found}");
}

#[test]
fn the_unobserved_count_and_row_walk_use_the_same_index() {
    let found = plan(&statements::unobserved_count());
    assert!(found.contains("facts_unobserved"), "{found}");
    for has_after in [false, true] {
        let found = plan(&statements::unobserved_rows(has_after));
        assert!(found.contains("facts_unobserved"), "{found}");
        assert!(!found.contains("TEMP B-TREE"), "{found}");
    }
}

#[test]
fn a_record_is_read_by_its_key() {
    let found = plan(&statements::record());
    assert!(
        found.contains("PRIMARY KEY") || found.contains("autoindex"),
        "{found}"
    );
}

#[test]
fn the_damage_check_uses_the_partial_damage_index() {
    let found = plan("SELECT EXISTS(SELECT 1 FROM task_facts WHERE valid=0)");
    assert!(found.contains("facts_damaged"), "{found}");
}

#[test]
fn the_related_candidate_lookup_uses_the_pgid_and_pid_indexes() {
    for ancestors in [0, 3] {
        let found = plan(&statements::related_callers(ancestors));
        assert!(found.contains("processes_pgid"), "{found}");
        if ancestors > 0 {
            assert!(found.contains("processes_pid"), "{found}");
        }
        assert!(!found.contains("SCAN p"), "{found}");
    }
}
