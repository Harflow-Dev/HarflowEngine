#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Shared oracle for the `commit_sequence` assertions of the crash-stage and in-process
//! tests. The row is read through an independent read-only SQLite connection.
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

/// The committed `commit_sequence` value of the store at `root`.
pub(crate) fn sequence(root: &Path) -> i64 {
    let db =
        Connection::open_with_flags(root.join("store.sqlite3"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    db.query_row(
        "SELECT value FROM commit_sequence WHERE singleton=1",
        [],
        |row| row.get(0),
    )
    .unwrap()
}

/// The expected value at a crash stage: a `before` stage leaves the baseline, an `after`
/// stage is exactly one commit ahead.
pub(crate) fn expected(baseline: i64, hook: &str) -> i64 {
    if hook.contains("after") {
        baseline + 1
    } else {
        baseline
    }
}
