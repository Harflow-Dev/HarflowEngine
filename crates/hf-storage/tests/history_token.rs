#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The change token, the foreign cursor binding and the recent event bodies of the ctl snapshot.
mod common;
mod fixture_support;
use fixture_support::{A, B, World, done, event, later, tid, world};
use hf_domain::{
    ctl::history::LogObject,
    event_read::{CaptureState, LogFacts},
};
use hf_storage::{Error, FixtureLine};

fn with_snapshot<T>(
    w: &World,
    action: impl FnOnce(&hf_storage::TaskSnapshot<'_>) -> Result<T, Error>,
) -> Result<T, Error> {
    w.session().snapshot(later(30), action)
}

fn logs(status: &str, exit: Option<i64>, drained: bool, capture: CaptureState) -> LogObject {
    let stream = LogFacts {
        task_status: None,
        available: true,
        capture,
        output_drained: None,
        eof_finished_seq: None,
        within_grace: false,
    };
    LogObject::derive(status, exit, drained, &stream)
}

fn token(w: &World, id: &str) -> String {
    with_snapshot(w, |snapshot| {
        let record = snapshot.record(&tid(id))?.unwrap();
        let log = logs(
            &record.facts.status,
            record.facts.exit_code,
            record.facts.output_drained,
            CaptureState::Unknown,
        );
        snapshot.event_change_token(&record, &log)
    })
    .unwrap()
}

#[test]
fn the_token_is_stable_and_changes_with_the_log_the_record_and_the_store() {
    let w = world();
    w.add(&done(A));
    for n in 0..3 {
        w.append(A, &event(n, "notice"));
    }
    let before = token(&w, A);
    assert_eq!(token(&w, A), before);
    assert_eq!(before.len(), 64);
    assert!(
        before
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    w.append(A, &event(3, "notice"));
    let appended = token(&w, A);
    assert_ne!(appended, before);
    w.append(A, &FixtureLine::Partial { len: 7 });
    let partial = token(&w, A);
    assert_ne!(partial, appended);
    w.raw()
        .execute("UPDATE task_facts SET title='renamed' WHERE id=?1", [A])
        .unwrap();
    assert_ne!(token(&w, A), partial, "a record-only change");
    let other = world();
    other.add(&done(A));
    for n in 0..3 {
        other.append(A, &event(n, "notice"));
    }
    assert_ne!(token(&other, A), before, "another store, the same inputs");
}

#[test]
fn the_token_binds_the_log_object_and_a_missing_stream() {
    let w = world();
    w.add(&done(A));
    w.add(&done(B));
    let bare = token(&w, B);
    w.append(B, &event(0, "notice"));
    assert_ne!(token(&w, B), bare);
    let token_of = |capture| {
        with_snapshot(&w, |s| {
            let record = s.record(&tid(A))?.unwrap();
            s.event_change_token(&record, &logs("completed", Some(0), true, capture))
        })
        .unwrap()
    };
    let observed = logs("completed", Some(0), true, CaptureState::Live);
    let drained_false = logs("completed", Some(0), false, CaptureState::Live);
    assert_ne!(observed.canonical(), drained_false.canonical());
    let running = logs("running", None, false, CaptureState::Live);
    assert_ne!(running.canonical(), observed.canonical());
    assert_eq!(
        token_of(CaptureState::Live),
        token_of(CaptureState::Unknown),
        "a derived record decides, not the stream"
    );
}

#[test]
fn the_foreign_cursor_binding_uses_the_references_digests() {
    // sha256 of the task id text, and of json.dumps(sorted kinds), from the reference module
    // (event_pages.py of HarflowMCP a426c5a) run on these inputs.
    let task_a = "303617b9730210ef3c86c52dc2aecc4dce54aaca6af8c8b0f4ceec9ecc54e57e";
    let task_b = "00f765af1c54eb24e437746c4f64b5841490757b647bf3a392b042f872ad7090";
    let default = "ce389cee2f6f5a4f4a89d9cf5323184abcdf96994b7ef92b93881f82353b84b1";
    let all = "bbd5927ab0d27ec617257d38bd4e40986341743da0cebb55415253e2d2e67125";
    let pair = "26b76a4ec8c5c557f8fa1d381dd25bb28bff9ce649ff8135c0f2a4ca7ec20ce2";
    let w = world();
    let kinds = |names: &[&str]| names.iter().map(|k| k.to_string()).collect::<Vec<_>>();
    let default_kinds = hf_domain::event_read::EventFilter::Default.kinds();
    let all_kinds = hf_domain::event_read::EventFilter::All.kinds();
    with_snapshot(&w, |s| {
        assert!(s.foreign_cursor_binds(&tid(A), &default_kinds, task_a, default));
        assert!(s.foreign_cursor_binds(&tid(A), &all_kinds, task_a, all));
        assert!(s.foreign_cursor_binds(
            &tid(A),
            &kinds(&["assistant_text", "notice"]),
            task_a,
            pair
        ));
        assert!(!s.foreign_cursor_binds(&tid(A), &default_kinds, task_b, default));
        assert!(!s.foreign_cursor_binds(&tid(B), &default_kinds, task_a, default));
        assert!(!s.foreign_cursor_binds(&tid(A), &all_kinds, task_a, default));
        assert!(!s.foreign_cursor_binds(&tid(A), &default_kinds, task_a, all));
        assert!(
            !s.foreign_cursor_binds(&tid(A), &kinds(&["notice", "assistant_text"]), task_a, pair),
            "the list is taken as sorted"
        );
        Ok(())
    })
    .unwrap();
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

/// A root whose limits admit a log of more than a megabyte.
fn roomy() -> World {
    let scratch = common::Fixture::new();
    let root = scratch.path().join("store");
    let limits = hf_storage::Limits {
        max_rows: 1_000,
        max_bytes: 16 * 1024 * 1024,
    };
    let store =
        hf_storage::Store::initialize_fixture(&root, limits, later(5), common::lease::lease_port())
            .unwrap();
    let all = hf_domain::task_read::Coverage {
        discovery_complete: true,
        ownership_complete: true,
        source_complete: true,
    };
    store
        .replace_task_fixture(
            &hf_storage::TaskFixture {
                rows: Vec::new(),
                launches: Vec::new(),
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

#[test]
fn recent_bodies_are_the_valid_records_of_the_last_megabyte_oldest_first() {
    let w = roomy();
    w.add(&done(A));
    let mut lengths = Vec::new();
    for seq in 0..100 {
        let line = padded(seq, 15_000);
        if let FixtureLine::Event { body, .. } = &line {
            lengths.push(body.len() as u64 + 1);
        }
        w.append(A, &line);
    }
    let limits = hf_storage::Limits {
        max_rows: 1_000,
        max_bytes: 16 * 1024 * 1024,
    };
    let session =
        hf_storage::ReadSession::open(&w.root, limits, later(30), common::lease::lease_port())
            .unwrap();
    let bodies = session
        .snapshot(later(30), |s| s.recent_event_bodies(&tid(A)))
        .unwrap();
    let end: u64 = lengths.iter().sum();
    let from = end - 1_048_576;
    let mut start = 0;
    let mut wanted = Vec::new();
    for (seq, len) in lengths.iter().enumerate() {
        if start >= from {
            wanted.push(seq);
        }
        start += len;
    }
    let seqs: Vec<usize> = bodies
        .iter()
        .map(|b| {
            let text = String::from_utf8(b.clone()).unwrap();
            let rest = text.strip_prefix("{\"seq\":").unwrap();
            rest.split(',').next().unwrap().parse().unwrap()
        })
        .collect();
    assert_eq!(seqs, wanted);
    assert!(seqs.len() < 100 && seqs.len() > 60);
}

#[test]
fn recent_bodies_skip_unreadable_lines_and_oversized_bodies_and_need_a_stream() {
    let w = world();
    w.add(&done(A));
    w.add(&done(B));
    assert!(
        with_snapshot(&w, |s| s.recent_event_bodies(&tid(B)))
            .unwrap()
            .is_empty()
    );
    w.append(A, &event(0, "notice"));
    w.append(A, &FixtureLine::Malformed { len: 9 });
    w.append(A, &event(1, "notice"));
    w.append(A, &event(2, "notice"));
    w.raw()
        .execute(
            "UPDATE event_records SET body=zeroblob(17000) WHERE task=?1 AND seq=1",
            [A],
        )
        .unwrap();
    let bodies = with_snapshot(&w, |s| s.recent_event_bodies(&tid(A))).unwrap();
    assert_eq!(bodies.len(), 2);
    assert!(
        String::from_utf8(bodies[0].clone())
            .unwrap()
            .contains("\"seq\":0")
    );
    assert!(
        String::from_utf8(bodies[1].clone())
            .unwrap()
            .contains("\"seq\":2")
    );
    w.raw()
        .execute("UPDATE event_streams SET available=0 WHERE task=?1", [A])
        .unwrap();
    assert!(
        with_snapshot(&w, |s| s.recent_event_bodies(&tid(A)))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_event_page_validates_its_query_before_reading() {
    let w = world();
    w.add(&done(A));
    let query = hf_domain::event_read::EventQuery {
        task: tid(A),
        limit: 0,
        filter: hf_domain::event_read::EventFilter::Default,
        from_oldest: false,
        cursor: None,
    };
    let outcome = with_snapshot(&w, |s| s.event_page(&query));
    assert!(
        matches!(outcome, Err(Error::InvalidEventQuery)),
        "{outcome:?}"
    );
}
