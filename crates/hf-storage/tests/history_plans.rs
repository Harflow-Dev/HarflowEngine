#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Query plans of the history reads, taken from the statements the snapshot runs, so a dropped
//! index or a changed predicate fails here. None may scan a table or sort.
mod common;
mod fixture_support;
use fixture_support::{A, done, world};
use hf_storage::statements;
use rusqlite::types::Value;

fn plan(sql: &str) -> String {
    let w = world();
    w.add(&done(A));
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

fn searched_by_key(found: &str) {
    assert!(!found.contains("SCAN"), "{found}");
    assert!(!found.contains("TEMP B-TREE"), "{found}");
    assert!(
        found.contains("PRIMARY KEY") || found.contains("autoindex") || found.contains("INDEX"),
        "{found}"
    );
}

#[test]
fn the_prompt_statements_read_by_key() {
    for sql in [
        statements::prompt_meta(),
        statements::prompt_text(),
        statements::prompt_blob(),
    ] {
        searched_by_key(&plan(sql));
    }
}

#[test]
fn the_manifest_and_the_continuation_are_read_by_key() {
    let found = plan(statements::manifest());
    assert!(found.contains("SEARCH m USING INDEX"), "{found}");
    assert!(
        found.contains("SEARCH i USING COVERING INDEX"),
        "the count walks the item index: {found}"
    );
    assert!(
        !found.contains("SCAN i") && !found.contains("TEMP B-TREE"),
        "{found}"
    );
    searched_by_key(&plan(statements::continuation()));
}

#[test]
fn the_items_come_in_ordinal_order_from_the_primary_key_without_a_sort() {
    let found = plan(statements::artifact_items());
    searched_by_key(&found);
    assert!(found.contains("artifact_items"), "{found}");
}

#[test]
fn the_recent_bodies_walk_the_start_index_newest_first_without_a_sort() {
    let found = plan(statements::recent_bodies());
    assert!(found.contains("event_starts"), "{found}");
    assert!(!found.contains("SCAN"), "{found}");
    assert!(!found.contains("TEMP B-TREE"), "{found}");
}
