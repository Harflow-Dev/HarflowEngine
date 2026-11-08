#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
mod feed_support;
mod read_session_support;
#[allow(unused_imports)]
use common::{deadline, default_limits};
#[allow(unused_imports)]
use feed_support::*;
#[allow(unused_imports)]
use hf_application::ctl::task_reads::ScanRow;
#[allow(unused_imports)]
use hf_domain::{
    ctl::statistics::{GenerationEntry, StatFacts},
    event_read::CaptureState,
    task_read::{Coverage, TaskId},
};
#[allow(unused_imports)]
use hf_storage::{Error, FixtureLine, Limits, ReadSession, Store};
#[allow(unused_imports)]
use read_session_support::edit_main;
#[allow(unused_imports)]
use sha2::{Digest, Sha256};
#[allow(unused_imports)]
use std::time::{Duration, Instant};

#[test]
fn feed_sources_come_in_byte_order_after_a_cursor_and_within_the_limit() {
    let root = root(
        ["b", "Z", "a1", "a-b", "a_b", "9"]
            .iter()
            .map(|n| task(n))
            .collect(),
    );
    let ids = |after: &str, limit| {
        read(&root, host(), |s| s.feed_sources(after, limit))
            .unwrap()
            .into_iter()
            .map(|f| f.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids("", 100), ["9", "Z", "a-b", "a1", "a_b", "b"]);
    assert_eq!(ids("Z", 2), ["a-b", "a1"]);
    assert_eq!(ids("b", 5), Vec::<String>::new());
    assert_eq!(ids("a1", 1), ["a_b"]);
}

#[test]
fn a_source_carries_its_facts_and_the_stored_blob_bytes_and_a_title_cut_at_200() {
    let mut long = task("long");
    long.facts.title = Some("\u{1f600}".repeat(300));
    long.header = vec![b'h'; 1500];
    long.status = vec![b's'; 700];
    let root = root(vec![long, task("plain")]);
    let found = read(&root, host(), |s| s.feed_source("long"))
        .unwrap()
        .unwrap();
    assert_eq!(
        (found.status.as_str(), found.exit_code, found.metadata_bytes),
        ("completed", Some(0), 2200)
    );
    assert_eq!(found.title.unwrap().chars().count(), 200);
    assert_eq!(
        (found.started_at_us, found.finished_at_us),
        (1_768_039_200_000_000, Some(1_768_039_500_000_000))
    );
    assert!(
        read(&root, host(), |s| s.feed_source("nothing"))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        read(&root, host(), |s| s.feed_source("plain"))
            .unwrap()
            .unwrap()
            .metadata_bytes,
        19 + 2
    );
}

#[test]
fn damaged_tasks_are_no_sources_and_no_scan_rows() {
    let root = root(names(3));
    edit_main(
        &root.path,
        "UPDATE task_facts SET valid=0, backend=NULL, repo_path=NULL, session_id=NULL, status=NULL, exit_code=NULL, started_at=NULL, started_at_us=NULL, finished_at=NULL, finished_at_us=NULL, output_drained=NULL, title=NULL, model=NULL, continuation_of=NULL, chain_root=NULL WHERE id='t0001'",
    );
    assert!(
        read(&root, host(), |s| s.feed_source("t0001"))
            .unwrap()
            .is_none()
    );
    let sources = read(&root, host(), |s| s.feed_sources("", 10)).unwrap();
    assert_eq!(
        sources.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(),
        ["t0000", "t0002"]
    );
    let rows = read(&root, host(), |s| {
        s.statistics_batch(None, deadline()).map(|b| b.rows)
    })
    .unwrap();
    assert_eq!(
        rows.iter().map(ScanRow::id).collect::<Vec<_>>(),
        ["t0000", "t0002"]
    );
}

#[test]
fn the_stream_capture_is_each_stored_code_and_nothing_for_an_unknown_one_or_no_stream() {
    let task = id("t0000");
    let bare = root(names(1));
    assert_eq!(
        read(&bare, host(), |s| s.stream_capture(&task)).unwrap(),
        None
    );
    let root = root_with(names(1), true);
    for (code, wanted) in [
        (0, Some(CaptureState::Live)),
        (1, Some(CaptureState::LostWithOwner)),
        (2, Some(CaptureState::ObservedTerminal)),
        (3, Some(CaptureState::Unknown)),
        (9, None),
    ] {
        edit_main(
            &root.path,
            &format!("UPDATE event_streams SET capture={code}"),
        );
        assert_eq!(
            read(&root, host(), |s| s.stream_capture(&task)).unwrap(),
            wanted,
            "{code}"
        );
    }
}

#[test]
fn the_scan_walks_in_batches_of_200_by_keyset_and_ends_with_an_empty_batch() {
    let root = root(names(450));
    let sizes = read(&root, host(), |s| {
        let mut after: Option<String> = None;
        let mut sizes = Vec::new();
        let mut seen = Vec::new();
        loop {
            let batch = s.statistics_batch(after.as_deref(), deadline())?.rows;
            let Some(last) = batch.last() else { break };
            after = Some(last.id().to_string());
            sizes.push(batch.len());
            seen.extend(batch.iter().map(|r| r.id().to_string()));
        }
        assert_eq!(
            seen,
            (0..450).map(|n| format!("t{n:04}")).collect::<Vec<_>>()
        );
        Ok(sizes)
    })
    .unwrap();
    assert_eq!(sizes, [200, 200, 50]);
}

#[test]
fn a_scan_row_holds_every_fact_the_projection_reads() {
    let mut one = task("c1");
    one.facts.model = Some("m".into());
    one.facts.continuation_of = Some(id("p1"));
    one.facts.chain_root = Some(id("p1"));
    one.facts.title = None;
    let root = root(vec![one]);
    let rows = read(&root, host(), |s| {
        s.statistics_batch(None, deadline()).map(|b| b.rows)
    })
    .unwrap();
    let found = facts_of(&rows[0]);
    assert_eq!(
        (
            found.status.as_str(),
            found.backend.as_str(),
            found.repo_path.as_str()
        ),
        ("completed", "claude", "/repo")
    );
    assert_eq!(
        (
            found.model.as_deref(),
            found.title.as_deref(),
            found.exit_code
        ),
        (Some("m"), None, Some(0))
    );
    assert_eq!(
        (
            found.continuation_of.as_ref().map(TaskId::as_str),
            found.chain_root.as_ref().map(TaskId::as_str)
        ),
        (Some("p1"), Some("p1"))
    );
    assert_eq!(
        (
            found.started_at.as_str(),
            found.started_at_us,
            found.finished_at_us
        ),
        (
            "2026-01-10T10:00:00+00:00",
            1_768_039_200_000_000,
            Some(1_768_039_500_000_000)
        )
    );
}

#[test]
fn a_row_over_4_mib_or_over_the_query_budget_is_oversized_without_being_read() {
    let root = root(names(3));
    edit_main(
        &root.path,
        "UPDATE task_facts SET title=hex(zeroblob(2400000)) WHERE id='t0001'",
    );
    let rows = read(&root, host(), |s| {
        s.statistics_batch(None, deadline()).map(|b| b.rows)
    })
    .unwrap();
    assert!(
        matches!(&rows[1], ScanRow::Oversized(found) if found == "t0001"),
        "{rows:?}"
    );
    assert!(matches!(&rows[0], ScanRow::Facts(_)) && matches!(&rows[2], ScanRow::Facts(_)));
    edit_main(
        &root.path,
        "UPDATE task_facts SET title=hex(zeroblob(700000)) WHERE id='t0001'",
    );
    // Over the 1 MiB query budget (and under 4 MiB) the row is oversized too: the known limitation.
    let rows = read(&root, host(), |s| {
        s.statistics_batch(None, deadline()).map(|b| b.rows)
    })
    .unwrap();
    assert!(
        matches!(&rows[1], ScanRow::Oversized(found) if found == "t0001"),
        "{rows:?}"
    );
    assert_eq!(rows.len(), 3);
}

#[test]
fn the_text_of_the_time_columns_counts_toward_the_budget_before_it_is_copied() {
    for column in ["started_at", "finished_at"] {
        let root = root(names(2));
        edit_main(
            &root.path,
            &format!("UPDATE task_facts SET {column}=hex(zeroblob(700000)) WHERE id='t0001'"),
        );
        let rows = read(&root, host(), |s| {
            s.statistics_batch(None, deadline()).map(|b| b.rows)
        })
        .unwrap();
        assert!(
            matches!(&rows[1], ScanRow::Oversized(found) if found == "t0001"),
            "{column}: {rows:?}"
        );
        assert!(matches!(&rows[0], ScanRow::Facts(_)));
    }
}

#[test]
fn a_batch_ends_before_the_row_that_would_pass_the_budget_and_counts_bytes_not_characters() {
    let root = root(names(9));
    // 3,000 three-byte characters: 9,000 bytes in each row.
    edit_main(
        &root.path,
        &format!(
            "UPDATE task_facts SET title=replace(hex(zeroblob(500)), '0', '{}')",
            "\u{4e2d}\u{4e2d}\u{4e2d}"
        ),
    );
    let limits = Limits {
        max_rows: 201,
        max_bytes: 10_000,
    };
    let (sizes, seen) = read(&root, limits, |s| {
        let mut after: Option<String> = None;
        let mut sizes = Vec::new();
        let mut seen = Vec::new();
        loop {
            let batch = s.statistics_batch(after.as_deref(), deadline())?.rows;
            let Some(last) = batch.last() else { break };
            after = Some(last.id().to_string());
            sizes.push(batch.len());
            seen.extend(batch.iter().map(|r| r.id().to_string()));
        }
        Ok((sizes, seen))
    })
    .unwrap();
    assert_eq!(seen.len(), 9);
    // Each row is about 9,000 bytes (3,000 characters): two never fit 10,000 bytes, three would by characters.
    assert_eq!(sizes, [1; 9]);
}

#[test]
fn a_deadline_that_has_passed_returns_an_expired_empty_batch_not_an_error_or_an_end_of_scan() {
    let root = root(names(3));
    let past = Instant::now() - Duration::from_secs(1);
    let batch = read(&root, host(), |s| s.statistics_batch(None, past)).unwrap();
    assert!(batch.expired);
    assert!(batch.rows.is_empty());
    let fine = read(&root, host(), |s| s.statistics_batch(None, deadline())).unwrap();
    assert!(!fine.expired);
    assert_eq!(fine.rows.len(), 3);
}

#[test]
fn a_deadline_inside_a_batch_keeps_the_rows_already_read() {
    let root = root(names(200));
    let mut partial = None;
    let mut micros = 5u64;
    while micros < 200_000 && partial.is_none() {
        // The deadline is set inside the snapshot, just before the batch is asked for.
        let batch = read(&root, host(), |s| {
            s.statistics_batch(None, Instant::now() + Duration::from_micros(micros))
        })
        .unwrap();
        if batch.expired && !batch.rows.is_empty() {
            partial = Some(batch);
        }
        micros += micros / 4 + 1;
    }
    let batch = partial.expect("some deadline falls inside the batch");
    assert!(batch.expired);
    assert!(batch.rows.len() < 200, "{}", batch.rows.len());
    let ids: Vec<&str> = batch.rows.iter().map(ScanRow::id).collect();
    let wanted: Vec<String> = (0..ids.len()).map(|n| format!("t{n:04}")).collect();
    assert_eq!(ids, wanted, "the kept rows are the first ones, in order");
}
