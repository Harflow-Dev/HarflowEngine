#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The recent event bodies, the workflow-builder fact and the recorded process in the change
//! token, read through the ctl snapshot of a real root.
mod common;
mod fixture_support;
use fixture_support::{A, B, World, done, event, identity, later, tid, world};
use hf_domain::{
    ctl::history::LogObject,
    event_read::{CaptureState, LogFacts},
};
use hf_storage::{Error, FixtureLine, FixtureProcess, Limits, ReadSession, Store, TaskFixture};

const WINDOW: u64 = 1_048_576;

fn limits() -> Limits {
    Limits {
        max_rows: 5_000,
        max_bytes: 16 * 1024 * 1024,
    }
}

/// A root whose limits admit a log of more than a megabyte and thousands of lines.
fn roomy() -> World {
    let scratch = common::Fixture::new();
    let root = scratch.path().join("store");
    let store =
        Store::initialize_fixture(&root, limits(), later(5), common::lease::lease_port()).unwrap();
    let all = hf_domain::task_read::Coverage {
        discovery_complete: true,
        ownership_complete: true,
        source_complete: true,
    };
    store
        .replace_task_fixture(
            &TaskFixture {
                rows: Vec::new(),
                pending: Vec::new(),
                coverage: all,
            },
            later(5),
        )
        .unwrap();
    World {
        scratch,
        root,
        store,
    }
}

fn recent(w: &World, id: &str) -> Result<Vec<Vec<u8>>, Error> {
    let session =
        ReadSession::open(&w.root, limits(), later(30), common::lease::lease_port()).unwrap();
    session.snapshot(later(30), |s| s.recent_event_bodies(&tid(id)))
}

fn padded(seq: u64, pad: usize) -> FixtureLine {
    FixtureLine::Event {
        seq,
        kind: "notice".into(),
        body: format!(
            "{{\"seq\":{seq},\"kind\":\"notice\",\"pad\":\"{}\"}}",
            "p".repeat(pad)
        )
        .into_bytes(),
    }
}

fn seqs(bodies: &[Vec<u8>]) -> Vec<u64> {
    bodies
        .iter()
        .map(|body| {
            let text = String::from_utf8(body.clone()).unwrap();
            text.strip_prefix("{\"seq\":")
                .unwrap()
                .split(',')
                .next()
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect()
}

fn big_log(w: &World, id: &str) {
    w.add(&done(id));
    for seq in 0..100 {
        w.append(id, &padded(seq, 15_000));
    }
}

#[test]
fn the_bodies_kept_never_pass_the_window_in_real_bytes() {
    let w = roomy();
    big_log(&w, A);
    let bodies = recent(&w, A).unwrap();
    let real: u64 = bodies.iter().map(|b| b.len() as u64 + 1).sum();
    assert!(real <= WINDOW, "{real}");
    assert!(real > WINDOW - 16_100, "the walk fills the window: {real}");
    let wanted = seqs(&bodies);
    assert_eq!(wanted.last(), Some(&99));
    assert!(
        wanted.windows(2).all(|pair| pair[1] == pair[0] + 1),
        "newest bodies, oldest first, no gap"
    );
}

#[test]
fn an_incomplete_tail_does_not_move_the_window() {
    let plain = roomy();
    big_log(&plain, A);
    let tailed = roomy();
    big_log(&tailed, A);
    tailed.append(A, &FixtureLine::Partial { len: 500_000 });
    let without = recent(&plain, A).unwrap();
    assert_eq!(recent(&tailed, A).unwrap(), without);
    assert!(without.len() > 60);
}

#[test]
fn bodies_whose_real_bytes_pass_the_window_together_stop_the_walk() {
    // The fixture writer only makes contiguous records, whose bodies cannot add up to more than
    // the window; overlapping spans in the store can, and the walk then stops at the budget.
    let w = roomy();
    big_log(&w, A);
    let db = w.raw();
    db.execute(
        "UPDATE event_streams SET complete_end=2000000 WHERE task=?1",
        [A],
    )
    .unwrap();
    db.execute("UPDATE event_records SET start=1000000+seq*1000+1000000*0, end=1000000+seq*1000+15000 WHERE task=?1", [A]).unwrap();
    let length: i64 = db
        .query_row(
            "SELECT length(body) FROM event_records WHERE task=?1 AND seq=0",
            [A],
            |r| r.get(0),
        )
        .unwrap();
    let bodies = recent(&w, A).unwrap();
    let fits = WINDOW / length as u64;
    assert!(fits < 100);
    assert_eq!(bodies.len() as u64, fits);
    assert_eq!(seqs(&bodies).last(), Some(&99));
}

#[test]
fn a_long_run_of_small_records_is_read_to_the_start_of_the_window() {
    let w = roomy();
    w.add(&done(A));
    w.append(A, &event(0, "notice"));
    for seq in 1..=1_500 {
        w.append(A, &event(seq, "usage"));
    }
    let bodies = recent(&w, A).unwrap();
    assert_eq!(
        bodies.len(),
        1_501,
        "past the 128 rows of an ordinary query"
    );
    assert_eq!(seqs_of_kind(&bodies, 0), 1);
}

fn seqs_of_kind(bodies: &[Vec<u8>], seq: u64) -> usize {
    bodies
        .iter()
        .filter(|b| String::from_utf8_lossy(b).contains(&format!("\"seq\":{seq},")))
        .count()
}

#[test]
fn malformed_lines_between_events_are_left_out() {
    let w = roomy();
    w.add(&done(A));
    for seq in 0..3 {
        w.append(A, &padded(seq, 100));
        w.append(A, &FixtureLine::Malformed { len: 9 });
    }
    let bodies = recent(&w, A).unwrap();
    assert_eq!(seqs(&bodies), [0, 1, 2]);
}

#[test]
fn a_bad_stream_row_gives_no_bodies_and_a_real_failure_stays_an_error() {
    let w = roomy();
    w.add(&done(A));
    w.append(A, &event(0, "notice"));
    for statement in [
        "UPDATE event_streams SET generation='not hex' WHERE task=?1",
        "UPDATE event_streams SET capture=9 WHERE task=?1",
        "UPDATE event_streams SET complete_end=-1 WHERE task=?1",
    ] {
        let fresh = roomy();
        fresh.add(&done(A));
        fresh.append(A, &event(0, "notice"));
        fresh.raw().execute(statement, [A]).unwrap();
        assert_eq!(
            recent(&fresh, A).unwrap(),
            Vec::<Vec<u8>>::new(),
            "{statement}"
        );
    }
    let session =
        ReadSession::open(&w.root, limits(), later(30), common::lease::lease_port()).unwrap();
    let past = std::time::Instant::now() - std::time::Duration::from_secs(1);
    let outcome = session.snapshot(past, |s| s.recent_event_bodies(&tid(A)));
    assert!(
        outcome.is_err(),
        "an expired deadline is never an empty summary"
    );
}

/// `bool(value)` in Python, the reference's `if current.workflow_builder:`.
const TRUTHINESS: [(&str, bool); 22] = [
    ("true", true),
    ("1", true),
    ("2.5", true),
    ("-1", true),
    ("\"true\"", true),
    ("\"x\"", true),
    ("\"0\"", true),
    ("\" \"", true),
    ("[1]", true),
    ("[0]", true),
    ("[null]", true),
    ("{\"a\":1}", true),
    ("{\"a\":null}", true),
    ("1e-300", true),
    ("false", false),
    ("0", false),
    ("0.0", false),
    ("-0.0", false),
    ("\"\"", false),
    ("[]", false),
    ("{}", false),
    ("null", false),
];

fn builder_in(column: &str, flag: &str) -> bool {
    let fresh = world();
    fresh.add(&done(A));
    fresh
        .raw()
        .execute(&format!("UPDATE task_headers SET {column}=CAST('{{\"workflow_builder\":{flag}}}' AS BLOB) WHERE id=?1"), [A])
        .unwrap();
    fresh
        .session()
        .snapshot(later(30), |s| s.workflow_builder(&tid(A)))
        .unwrap()
}

#[test]
fn the_builder_flag_follows_python_truthiness_in_either_blob() {
    let w = world();
    w.add(&done(A));
    let read = |id: &str| {
        w.session()
            .snapshot(later(30), |s| s.workflow_builder(&tid(id)))
            .unwrap()
    };
    assert!(!read(A));
    assert!(!read(B), "no such task");
    for column in ["header", "status"] {
        for (flag, expected) in TRUTHINESS {
            assert_eq!(builder_in(column, flag), expected, "{column} {flag}");
        }
    }
    let absent = world();
    absent.add(&done(A));
    absent
        .raw()
        .execute(
            "UPDATE task_headers SET header=CAST('{\"other\":true}' AS BLOB) WHERE id=?1",
            [A],
        )
        .unwrap();
    assert!(
        !absent
            .session()
            .snapshot(later(30), |s| s.workflow_builder(&tid(A)))
            .unwrap()
    );
    let broken = world();
    broken.add(&done(A));
    broken
        .raw()
        .execute(
            "UPDATE task_headers SET header=CAST('not json' AS BLOB), status=NULL WHERE id=?1",
            [A],
        )
        .unwrap();
    assert!(
        !broken
            .session()
            .snapshot(later(30), |s| s.workflow_builder(&tid(A)))
            .unwrap()
    );
    let array = world();
    array.add(&done(A));
    array
        .raw()
        .execute(
            "UPDATE task_headers SET header=CAST('[true]' AS BLOB) WHERE id=?1",
            [A],
        )
        .unwrap();
    assert!(
        !array
            .session()
            .snapshot(later(30), |s| s.workflow_builder(&tid(A)))
            .unwrap(),
        "not an object"
    );
}

#[test]
fn blobs_over_the_query_budget_fail_the_read_and_never_answer_not_a_builder() {
    for (column, size) in [("header", 1_100_000usize), ("status", 1_100_000)] {
        let w = world();
        w.add(&done(A));
        let padding = "p".repeat(size);
        w.raw()
            .execute(
                &format!("UPDATE task_headers SET {column}=CAST(?1 AS BLOB) WHERE id=?2"),
                rusqlite::params![
                    format!("{{\"workflow_builder\":false,\"pad\":\"{padding}\"}}"),
                    A
                ],
            )
            .unwrap();
        let outcome = w
            .session()
            .snapshot(later(30), |s| s.workflow_builder(&tid(A)));
        assert!(
            matches!(outcome, Err(Error::LimitExceeded)),
            "{column}: {outcome:?}"
        );
    }
    let split = world();
    split.add(&done(A));
    let half = format!("{{\"pad\":\"{}\"}}", "p".repeat(600_000));
    split
        .raw()
        .execute(
            "UPDATE task_headers SET header=CAST(?1 AS BLOB), status=CAST(?1 AS BLOB) WHERE id=?2",
            rusqlite::params![half, A],
        )
        .unwrap();
    let outcome = split
        .session()
        .snapshot(later(30), |s| s.workflow_builder(&tid(A)));
    assert!(
        matches!(outcome, Err(Error::LimitExceeded)),
        "the two blobs together are the budget: {outcome:?}"
    );
    let ok = world();
    ok.add(&done(A));
    let near = format!("{{\"pad\":\"{}\"}}", "p".repeat(1_000_000));
    ok.raw()
        .execute(
            "UPDATE task_headers SET header=CAST(?1 AS BLOB) WHERE id=?2",
            rusqlite::params![near, A],
        )
        .unwrap();
    assert!(
        !ok.session()
            .snapshot(later(30), |s| s.workflow_builder(&tid(A)))
            .unwrap(),
        "just under the budget is served"
    );
}

fn token_of(w: &World) -> String {
    w.session()
        .snapshot(later(30), |s| {
            let record = s.record(&tid(A))?.unwrap();
            let stream = LogFacts {
                task_status: None,
                available: true,
                capture: CaptureState::Unknown,
                output_drained: None,
                eof_finished_seq: None,
                within_grace: false,
            };
            let log = LogObject::derive(
                &record.facts.status,
                record.facts.exit_code,
                record.facts.output_drained,
                &stream,
            );
            s.event_change_token(&record, &log)
        })
        .unwrap()
}

#[test]
fn the_recorded_pid_and_start_identity_are_part_of_the_token() {
    let w = world();
    let mut running = fixture_support::task(A, "running", None, false);
    running.process = Some(FixtureProcess {
        pid: 4_000,
        start: Some(identity(4_000)),
    });
    w.add(&running);
    let before = token_of(&w);
    assert_eq!(token_of(&w), before);
    for statement in [
        "UPDATE task_processes SET pid=4001 WHERE task=?1",
        "UPDATE task_processes SET start_micros=start_micros+1 WHERE task=?1",
        "UPDATE task_processes SET start_seconds=start_seconds+1 WHERE task=?1",
        "UPDATE task_processes SET boot_uuid='00000000-0000-4000-8000-000000000000' WHERE task=?1",
    ] {
        let fresh = world();
        fresh.add(&running);
        let first = token_of(&fresh);
        fresh.raw().execute(statement, [A]).unwrap();
        assert_ne!(token_of(&fresh), first, "{statement}");
    }
}
