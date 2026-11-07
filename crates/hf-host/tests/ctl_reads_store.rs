#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Store-backed ctl reads on roots the fixture port cannot seed: pending launches, very many unobserved rows and byte-limited batches.
mod ctl_reads_store_support;
use ctl_reads_store_support::*;
use hf_domain::ctl::task_rows::BatchQuery;
use hf_storage::{Limits, ReadSession};

#[test]
fn a_pending_launch_has_no_launch_state_and_is_dropped_from_explicit_ids() {
    let _one = exclusive();
    let scratch = scratch("pending");
    let root = scratch.0.join("store");
    let writer = store(&root);
    writer
        .add_task_fixture(&task(1, "completed", Some(0), 1, "/r"), later())
        .unwrap();
    writer.replace_pending_fixture(&[id(5)], later()).unwrap();
    drop(writer);
    let pending = id(5);
    let out = ctl(&root, &["status", pending.as_str()]);
    assert_eq!(out.status.code(), Some(1));
    let body = text(&out);
    assert!(body.contains("\"code\":\"task_pending_launch\""), "{body}");
    assert!(!body.contains("launch_state"), "{body}");
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
