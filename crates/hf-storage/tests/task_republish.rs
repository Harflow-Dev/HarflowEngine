#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `Store::republish_header`: terminal monotonicity of the recorded status, the facts
//! fallback, missing and damaged tasks, size bounds, and that a kept terminal status
//! commits nothing.
mod common;
mod fixture_support;
mod task_writer_support;
use fixture_support::{A, B, C, World, hex, later, quoted, tid, world};
use hf_domain::task_read::StoredObservation;
use hf_storage::{Error, Republished};
use task_writer_support::{HEADER_COLUMNS, done, published, put, rows, running};

fn observed(status: Option<&str>) -> StoredObservation {
    StoredObservation {
        recorded_status: status.map(str::to_owned),
        active: Some(status == Some("running")),
        exited: Some(true),
        drained: None,
        owned: Some(true),
    }
}

fn republish(w: &World, id: &str, status: Option<&str>) -> hf_storage::Result<Republished> {
    w.store.republish_header(
        &tid(id),
        b"new header",
        b"new status",
        &observed(status),
        later(5),
    )
}

/// Asserts the republish is kept: no row, generation, revision or sequence changes.
fn kept(w: &World, id: &str, status: Option<&str>) {
    let before = (rows(w, id), published(w));
    assert_eq!(
        republish(w, id, status).unwrap(),
        Republished::KeptTerminal,
        "{status:?}"
    );
    assert_eq!((rows(w, id), published(w)), before, "{status:?}");
}

/// Asserts the republish lands: the header row changes and exactly one publication commits.
fn landed(w: &World, id: &str, status: Option<&str>) {
    let (generation, watch, sequence) = published(w);
    let facts = rows(w, id)[0].clone();
    assert_eq!(
        republish(w, id, status).unwrap(),
        Republished::Landed,
        "{status:?}"
    );
    let after = published(w);
    assert_ne!(after.0, generation);
    assert_eq!((after.1, after.2), (watch + 1, sequence + 1), "{status:?}");
    let recorded = status.map_or("NULL".to_owned(), |s| format!("'{s}'"));
    let active = u8::from(status == Some("running"));
    assert_eq!(
        quoted(&w.db(), "task_headers", HEADER_COLUMNS, "id", id),
        format!(
            "1768039200000000|'session-{}'|{active}|{recorded}|1|NULL|1|{}|{}",
            &id[..8],
            hex(b"new header"),
            hex(b"new status")
        )
    );
    assert_eq!(rows(w, id)[0], facts, "the facts row is left unchanged");
}

#[test]
fn a_terminal_status_is_kept_over_a_non_terminal_or_absent_one() {
    for terminal in ["completed", "failed", "timed_out", "cancelled"] {
        let w = world();
        put(&w, &task_writer_support::create(A, terminal, Some(0), "p"));
        for status in [Some("running"), Some("queued"), Some("Completed"), None] {
            kept(&w, A, status);
        }
    }
}

#[test]
fn a_terminal_status_may_be_replaced_by_another_terminal_one() {
    let w = world();
    put(&w, &done(A));
    landed(&w, A, Some("failed"));
    landed(&w, A, Some("cancelled"));
    landed(&w, A, Some("cancelled"));
    kept(&w, A, Some("running"));
}

#[test]
fn a_non_terminal_status_may_be_replaced_by_anything() {
    for status in [Some("running"), Some("queued"), None, Some("timed_out")] {
        let w = world();
        put(&w, &running(A));
        landed(&w, A, status);
    }
    let w = world();
    put(&w, &running(A));
    landed(&w, A, Some("queued"));
    landed(&w, A, None);
    landed(&w, A, Some("running"));
    landed(&w, A, Some("completed"));
    kept(&w, A, Some("running"));
}

fn clear_recorded_status(w: &World, id: &str) {
    w.raw()
        .execute(
            "UPDATE task_headers SET recorded_status=NULL WHERE id=?1",
            [id],
        )
        .unwrap();
}

#[test]
fn a_missing_recorded_status_falls_back_to_the_facts_status() {
    let w = world();
    put(&w, &done(A));
    put(&w, &running(B));
    clear_recorded_status(&w, A);
    clear_recorded_status(&w, B);
    kept(&w, A, Some("running"));
    kept(&w, A, None);
    landed(&w, A, Some("failed"));
    landed(&w, B, Some("queued"));
}

#[test]
fn a_header_recorded_status_takes_precedence_over_the_facts_status() {
    let w = world();
    put(&w, &running(A));
    landed(&w, A, Some("completed"));
    kept(&w, A, Some("running"));
}

#[test]
fn a_missing_or_damaged_task_is_refused_and_nothing_commits() {
    let w = world();
    put(&w, &done(A));
    w.store.damage_task_fixture(&tid(A), later(5)).unwrap();
    put(&w, &done(B));
    w.store.damage_task_fixture(&tid(B), later(5)).unwrap();
    for id in [A, B, C] {
        let before = (rows(&w, id), published(&w));
        for status in [Some("completed"), Some("running"), None] {
            let error = republish(&w, id, status).unwrap_err();
            assert!(matches!(error, Error::TaskMissing), "{id}: {error:?}");
        }
        assert_eq!((rows(&w, id), published(&w)), before);
    }
}

#[test]
fn blobs_of_2048_bytes_land_and_2049_are_refused_before_any_lookup() {
    let w = world();
    put(&w, &running(A));
    let ok = vec![b'b'; 2048];
    let over = vec![b'b'; 2049];
    let run = |header: &[u8], status: &[u8], id: &str| {
        w.store.republish_header(
            &tid(id),
            header,
            status,
            &observed(Some("running")),
            later(5),
        )
    };
    assert_eq!(run(&ok, &ok, A).unwrap(), Republished::Landed);
    let before = (rows(&w, A), published(&w));
    for (header, status) in [(&over, &ok), (&ok, &over)] {
        for id in [A, C] {
            let error = run(header, status, id).unwrap_err();
            assert!(matches!(error, Error::LimitExceeded), "{id}: {error:?}");
        }
    }
    assert_eq!((rows(&w, A), published(&w)), before);
}

#[test]
fn a_recorded_status_of_256_bytes_lands_and_257_is_refused() {
    let w = world();
    put(&w, &running(A));
    let status = "\u{e9}".repeat(128);
    landed(&w, A, Some(&status));
    let before = (rows(&w, A), published(&w));
    let error = republish(&w, A, Some(&format!("{status}x"))).unwrap_err();
    assert!(matches!(error, Error::LimitExceeded), "{error:?}");
    assert_eq!((rows(&w, A), published(&w)), before);
}
