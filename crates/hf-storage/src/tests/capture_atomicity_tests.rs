#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod sequence;
mod support;
use rusqlite::Connection;
use support::{Actor, Fixture};

#[test]
fn independent_actor_sqlite_oracle_covers_progress_and_terminal_commit_cuts() {
    for (action, hook, committed) in [
        ("progress", "capture_before_commit", false),
        ("progress", "capture_after_commit", true),
        ("finish", "capture_before_terminal", false),
        ("finish", "capture_after_terminal", true),
    ] {
        let fixture = Fixture::new();
        let mut actor = Actor::spawn(&fixture, action, hook);
        actor.go();
        assert_eq!(
            actor.line(&format!("STAGE:{hook}")),
            format!("STAGE:{hook}")
        );
        let raw = fixture.readonly();
        let row = raw_state(&raw);
        let expected = match (action, committed) {
            ("progress", true) => (1, 3, 2, 0, 0, 0, 0, 0),
            ("finish", true) => (2, 0, 0, 1, 1, 0, 1, 0),
            ("finish", false) => (1, 0, 0, 1, 1, 0, 0, 0),
            _ => (0, 0, 0, 0, 0, 0, 0, 0),
        };
        assert_eq!(row, expected, "{action} at {hook}");
        assert_eq!(event_count(&raw), expected.0);
        if expected.0 > 0 {
            assert_eq!(last_event(&raw), expected, "event and snapshot at {hook}");
        }
        drop(raw);
        actor.kill_and_reap();
        let raw = fixture.readonly();
        let after_cut = raw_state(&raw);
        assert_eq!(after_cut, expected, "post-cut state at {hook}");
        assert_eq!(event_count(&raw), expected.0);
        if expected.0 > 0 {
            assert_eq!(last_event(&raw), expected, "post-cut event at {hook}");
        }
    }
}

type State = (i64, i64, i64, i64, i64, i64, i64, i64);
fn raw_state(raw: &Connection) -> State {
    raw.query_row(
        "SELECT revision,stdout,stderr,stdout_eof,stderr_eof,loss,exit_kind,exit_value FROM capture_records",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
    )
    .unwrap()
}
fn last_event(raw: &Connection) -> State {
    raw.query_row(
        "SELECT revision,stdout,stderr,stdout_eof,stderr_eof,loss,exit_kind,exit_value FROM capture_events ORDER BY revision DESC LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
    ).unwrap()
}
fn event_count(raw: &Connection) -> i64 {
    raw.query_row("SELECT count(*) FROM capture_events", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn capture_actor_child() {
    support::actor_child();
}
