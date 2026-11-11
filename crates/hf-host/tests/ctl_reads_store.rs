#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Store-backed ctl reads on roots the fixture port cannot seed: pending launches, very many unobserved rows and byte-limited batches.
mod ctl_reads_store_support;
use ctl_reads_store_support::*;
use hf_domain::ctl::task_rows::BatchQuery;
use hf_storage::{Limits, ReadSession};

#[test]
fn a_pending_launch_reports_its_launch_state_and_is_dropped_from_explicit_ids() {
    let _one = exclusive();
    let scratch = scratch("pending");
    let root = scratch.0.join("store");
    let writer = store(&root);
    writer
        .add_task_fixture(&task(1, "completed", Some(0), 1, "/r"), later())
        .unwrap();
    writer
        .replace_launch_fixture(
            &[hf_storage::LaunchRow::waiting_for_resources(id(5))],
            later(),
        )
        .unwrap();
    drop(writer);
    let pending = id(5);
    let out = ctl(&root, &["status", pending.as_str()]);
    assert_eq!(out.status.code(), Some(1));
    let body = text(&out);
    assert!(body.contains("\"code\":\"task_pending_launch\""), "{body}");
    assert!(
        body.contains("\"data\":{\"launch_state\":\"waiting_for_resources\"}"),
        "{body}"
    );
    let ids = format!("{},{}", pending.as_str(), id(1).as_str());
    let out = ctl(&root, &["task-list-page", "--task-ids", &ids]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let body = text(&out);
    assert!(body.contains(id(1).as_str()), "{body}");
    assert!(!body.contains(pending.as_str()), "{body}");
}

#[test]
fn over_4096_unobserved_rows_claim_no_count_and_refuse_nothing() {
    let _one = exclusive();
    let scratch = scratch("cap");
    for (name, total, claimed) in [("over", 4097, false), ("edge", 4096, true)] {
        let root = scratch.0.join(name);
        let writer = store(&root);
        for n in 0..total {
            writer
                .add_task_fixture(&task(n + 1, "running", None, 1, "/r"), later())
                .unwrap();
        }
        drop(writer);
        let out = ctl(&root, &["task-list-page", "--limit", "1"]);
        assert_eq!(out.status.code(), Some(0), "{name}: {}", text(&out));
        let body = text(&out);
        assert!(body.contains("\"history_incomplete\":false"), "{body}");
        assert!(!body.contains("authority_incomplete"), "{body}");
        if claimed {
            assert!(body.contains("\"total_active_root_count\":4096"), "{body}");
            assert!(
                body.contains("\"total_active_count\":null"),
                "uncertain rows: {body}"
            );
        } else {
            for key in [
                "total_active_count",
                "total_active_root_count",
                "total_attention_root_count",
            ] {
                assert!(body.contains(&format!("\"{key}\":null")), "{key}: {body}");
            }
        }
        assert!(body.contains("\"counts_complete\":false"), "{body}");
    }
}

#[test]
fn a_batch_that_reaches_the_byte_limit_continues_instead_of_failing() {
    let _one = exclusive();
    let scratch = scratch("bytes");
    let root = scratch.0.join("store");
    let writer = store(&root);
    for n in 1..=10 {
        writer
            .add_task_fixture(&task(n, "completed", Some(0), 1500, "/r"), later())
            .unwrap();
    }
    drop(writer);
    let limits = Limits {
        max_rows: 128,
        max_bytes: 8_000,
    };
    let session = ReadSession::open(&root, limits, later(), hf_platform::lease::shared()).unwrap();
    let mut seen: Vec<String> = Vec::new();
    let mut batches = 0;
    session
        .snapshot(later(), |snapshot| {
            let mut after = None;
            loop {
                let batch = snapshot.batch(&BatchQuery {
                    after: after.clone(),
                    max_rows: 100,
                    ..BatchQuery::default()
                })?;
                let Some(last) = batch.last() else { break };
                assert!(
                    batch.len() < 10 || batches == 0,
                    "a batch is cut at the byte limit"
                );
                after = Some((last.facts.started_at_us, last.facts.id.as_str().to_string()));
                seen.extend(batch.iter().map(|r| r.facts.id.as_str().to_string()));
                batches += 1;
            }
            Ok(())
        })
        .unwrap();
    assert!(batches > 1, "the limit must have cut the walk");
    let wanted: Vec<String> = (1..=10).rev().map(|n| id(n).as_str().to_string()).collect();
    assert_eq!(seen, wanted);
}

fn launch_root(
    scratch: &Scratch,
    name: &str,
    edit: impl Fn(&mut hf_storage::LaunchRow),
) -> std::path::PathBuf {
    let root = scratch.0.join(name);
    let writer = store(&root);
    writer
        .add_task_fixture(&task(1, "completed", Some(0), 1, "/r"), later())
        .unwrap();
    let mut launch = hf_storage::LaunchRow::waiting_for_resources(id(5));
    edit(&mut launch);
    let mut shadow = hf_storage::LaunchRow::waiting_for_resources(id(1));
    edit(&mut shadow);
    writer
        .replace_launch_fixture(&[launch, shadow], later())
        .unwrap();
    root
}

fn pending_data(root: &std::path::Path, args: &[&str]) -> String {
    let out = ctl(root, args);
    assert_eq!(out.status.code(), Some(1), "{args:?}: {}", text(&out));
    text(&out)
}

#[test]
fn not_started_reason_is_carried_only_when_the_receipt_has_one() {
    let _one = exclusive();
    let scratch = scratch("launch-reason");
    let five = id(5);
    let with = launch_root(&scratch, "with", |row| {
        row.state = Some("not_started".into());
        row.not_started_reason = Some("resource_timeout".into());
    });
    let body = pending_data(&with, &["status", five.as_str()]);
    assert!(
        body.contains(
            "\"data\":{\"launch_state\":\"not_started\",\"not_started_reason\":\"resource_timeout\"}"
        ),
        "{body}"
    );
    assert!(body.contains("its launch is not_started;"), "{body}");
    for (name, reason) in [("none", None), ("empty", Some(String::new()))] {
        let root = launch_root(&scratch, name, |row| {
            row.state = Some("not_started".into());
            row.not_started_reason = reason.clone();
        });
        let body = pending_data(&root, &["status", five.as_str()]);
        assert!(
            body.contains("\"data\":{\"launch_state\":\"not_started\"}"),
            "{name}: {body}"
        );
        assert!(!body.contains("not_started_reason"), "{name}: {body}");
    }
}

#[test]
fn every_unfinished_state_of_a_pending_kind_launch_reports_itself() {
    let _one = exclusive();
    let scratch = scratch("launch-states");
    for state in ["reserved", "claimed", "waiting_for_resources", "granted"] {
        let root = launch_root(&scratch, state, |row| row.state = Some(state.into()));
        let body = pending_data(&root, &["status", id(5).as_str()]);
        assert!(
            body.contains(&format!("\"data\":{{\"launch_state\":\"{state}\"}}")),
            "{state}: {body}"
        );
    }
    let worktree = launch_root(&scratch, "worktree", |row| {
        row.resource_names = None;
        row.has_resources = Some(false);
        row.has_worktree = Some(true);
    });
    let body = pending_data(&worktree, &["status", id(5).as_str()]);
    assert!(body.contains("\"code\":\"task_pending_launch\""), "{body}");
}

#[test]
fn spawned_settled_and_plain_launches_are_unknown_tasks() {
    let _one = exclusive();
    let scratch = scratch("launch-filtered");
    let plain = |row: &mut hf_storage::LaunchRow| {
        row.resource_names = None;
        row.has_resources = Some(false);
        row.has_worktree = Some(false);
    };
    for (name, edit) in [
        (
            "spawned",
            Box::new(|r: &mut hf_storage::LaunchRow| r.state = Some("spawned".into()))
                as Box<dyn Fn(&mut _)>,
        ),
        (
            "settled",
            Box::new(|r: &mut hf_storage::LaunchRow| r.state = Some("settled".into())),
        ),
        ("plain", Box::new(plain)),
    ] {
        let root = launch_root(&scratch, name, edit);
        let body = pending_data(&root, &["status", id(5).as_str()]);
        assert!(body.contains("\"code\":\"unknown_task\""), "{name}: {body}");
        assert!(!body.contains("launch_state"), "{name}: {body}");
    }
}

#[test]
fn an_ordinary_record_with_the_same_id_wins_over_its_launch() {
    let _one = exclusive();
    let scratch = scratch("launch-record");
    let root = launch_root(&scratch, "record", |_| {});
    let out = ctl(&root, &["status", id(1).as_str()]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let batch = ctl(&root, &["status-batch", "--task-ids", id(1).as_str()]);
    assert_eq!(batch.status.code(), Some(0), "{}", text(&batch));
    let body = text(&batch);
    assert!(body.contains("\"errors\":{}"), "{body}");
    assert!(!body.contains("task_pending_launch"), "{body}");
}

#[test]
fn a_batch_reports_the_launch_state_of_a_pending_id() {
    let _one = exclusive();
    let scratch = scratch("launch-batch");
    let root = launch_root(&scratch, "batch", |row| {
        row.state = Some("not_started".into());
        row.not_started_reason = Some("why".into());
    });
    let ids = format!("{},{}", id(5).as_str(), id(1).as_str());
    let out = ctl(&root, &["status-batch", "--task-ids", &ids]);
    let body = text(&out);
    assert_eq!(out.status.code(), Some(0), "{body}");
    assert!(body.contains("\"code\":\"task_pending_launch\""), "{body}");
    assert!(
        body.contains("\"launch_state\":\"not_started\"")
            && body.contains("\"not_started_reason\":\"why\""),
        "{body}"
    );
    assert!(body.contains(id(1).as_str()), "{body}");
}

/// Python `ctl.py:966` adds `not_started_reason` only when it is truthy, so an empty string
/// is omitted from the error data.
#[test]
fn an_empty_not_started_reason_is_omitted_from_the_launch_data() {
    let _one = exclusive();
    let scratch = scratch("launch-empty-reason");
    let root = launch_root(&scratch, "empty", |row| {
        row.state = Some("not_started".into());
        row.not_started_reason = Some(String::new());
    });
    let out = ctl(&root, &["status", id(5).as_str()]);
    let body = text(&out);
    assert!(
        body.contains("\"data\":{\"launch_state\":\"not_started\"}"),
        "{body}"
    );
}

#[test]
fn task_events_report_the_launch_state_and_reason() {
    let _one = exclusive();
    let scratch = scratch("launch-events");
    let root = launch_root(&scratch, "events", |row| {
        row.state = Some("not_started".into());
        row.not_started_reason = Some("why".into());
    });
    let body = pending_data(&root, &["task-events", id(5).as_str()]);
    assert!(
        body.contains("\"data\":{\"launch_state\":\"not_started\",\"not_started_reason\":\"why\"}"),
        "{body}"
    );
}
