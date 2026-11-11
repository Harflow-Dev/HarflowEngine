#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Header and page bounds, coverage, text limits and accounting of the store-backed ctl reads.
mod ctl_reads_store_support;
use ctl_reads_store_support::*;
use hf_domain::{
    ctl::task_rows::BatchQuery,
    task_read::{Coverage, TaskId},
};
use hf_storage::{FixtureTask, Limits, ReadSession, Store, TaskFixture};
use std::{path::Path, process::Output};

fn crafted(n: u32, header_extra: &str, status_extra: &str) -> FixtureTask {
    let mut made = task(n, "completed", Some(0), 1, "/r");
    made.header = format!(r#"{{"task_id":"{}"{header_extra}}}"#, id(n).as_str()).into_bytes();
    made.status = format!(r#"{{"task_id":"{}"{status_extra}}}"#, id(n).as_str()).into_bytes();
    made
}

fn notes(count: usize, width: usize) -> String {
    (0..count)
        .map(|k| format!(r#","note_{k}":"{}""#, "n".repeat(width)))
        .collect()
}

fn ids_in(body: &str) -> Vec<String> {
    body.match_indices("\"task_id\":\"")
        .map(|(at, found)| body[at + found.len()..][..36].to_string())
        .collect()
}

fn next_cursor(body: &str) -> Option<String> {
    let key = "\"next_cursor\":\"";
    let at = body.find(key)? + key.len();
    Some(body[at..][..body[at..].find('"')?].to_string())
}

#[test]
fn a_page_reaches_the_128_kib_bound_through_bounded_headers_and_the_walk_covers_all() {
    let _one = exclusive();
    let scratch = scratch("size");
    let root = scratch.0.join("store");
    let writer = store(&root);
    // Eight 200-character values survive the header cut, so every item is near 2 KiB.
    let extra = notes(8, 220);
    for n in 1..=100 {
        writer
            .add_task_fixture(&crafted(n, &extra, ""), later())
            .unwrap();
    }
    drop(writer);
    let (mut seen, mut cursor, mut pages) = (Vec::new(), None::<String>, 0);
    loop {
        let mut args = vec!["task-list-page"];
        if let Some(c) = cursor.as_deref() {
            args.extend(["--cursor", c]);
        }
        let out = ctl(&root, &args);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
        let body = text(&out);
        let items = &body[body.find("\"items\":[").unwrap()..body.find("\"next_cursor\"").unwrap()];
        assert!(
            items.len() <= 128 * 1024,
            "page items of {} bytes",
            items.len()
        );
        assert!(
            !items.contains(&"n".repeat(201)),
            "values are cut to 200 characters"
        );
        seen.extend(ids_in(items));
        pages += 1;
        cursor = next_cursor(&body);
        assert_eq!(cursor.is_some(), body.contains("\"has_more\":true"));
        if cursor.is_none() {
            break;
        }
        assert!(
            items.len() > 120_000,
            "a continuing page is filled: {}",
            items.len()
        );
    }
    assert!(pages > 1, "the input must reach the page bound");
    let mut wanted: Vec<String> = (1..=100)
        .rev()
        .map(|n| id(n).as_str().to_string())
        .collect();
    assert_eq!(seen.len(), 100);
    seen.sort();
    wanted.sort();
    assert_eq!(seen, wanted);
}

#[test]
fn a_header_that_cannot_be_bounded_falls_back_and_marks_history_and_counts() {
    let _one = exclusive();
    let scratch = scratch("fallback");
    let root = scratch.0.join("store");
    let writer = store(&root);
    // `workflow_id` is protected and never cut, so no cut can bring this header under 2 KiB.
    let heavy = format!(r#","workflow_id":"{}""#, "w".repeat(1900));
    writer
        .add_task_fixture(&crafted(1, &heavy, ""), later())
        .unwrap();
    writer
        .add_task_fixture(&crafted(2, "", ""), later())
        .unwrap();
    drop(writer);
    let out = ctl(&root, &["task-list-page"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let body = text(&out);
    assert!(body.contains("\"needs_direct_lookup\":true"), "{body}");
    assert!(
        body.contains("\"index_blocker\":\"header_metadata_budget\""),
        "{body}"
    );
    assert!(body.contains("\"history_incomplete\":true"), "{body}");
    assert!(body.contains("\"counts_complete\":false"), "{body}");
    for key in [
        "total_active_count",
        "total_active_root_count",
        "total_attention_root_count",
    ] {
        assert!(body.contains(&format!("\"{key}\":null")), "{key}: {body}");
    }
    assert!(body.contains(id(2).as_str()));
    let only = ctl(&root, &["task-list-page", "--task-ids", id(2).as_str()]);
    assert!(!text(&only).contains("needs_direct_lookup"));
    let status = ctl(&root, &["status", id(1).as_str()]);
    assert_eq!(status.status.code(), Some(0));
    assert!(
        !text(&status).contains("needs_direct_lookup"),
        "status is not bounded"
    );
}

fn gated(root: &Path) -> Vec<(Vec<String>, Output)> {
    let one = id(1);
    let one = one.as_str();
    [
        vec!["list"],
        vec!["status", one],
        vec!["status-batch", "--task-ids", one],
        vec!["task-list-page"],
        vec!["task-list-page", "--task-ids", one],
        vec!["task-list-page", "--predecessor-task-id", one],
    ]
    .into_iter()
    .map(|args| {
        let out = ctl(root, &args);
        (args.iter().map(|a| a.to_string()).collect(), out)
    })
    .collect()
}

/// What a damaged-or-incomplete store answers on each gated read, or why it did not.
fn damaged_answer(args: &[String], out: &Output) -> Option<String> {
    let body = text(out);
    let page = args[0] == "task-list-page" && !args.contains(&"--predecessor-task-id".to_string());
    let (code, needle) = if page {
        (0, "\"authority_incomplete\":true")
    } else if args[0] == "task-list-page" {
        (1, "\"code\":\"caller_undecidable\"")
    } else {
        (1, "\"code\":\"workflow_read_refused\"")
    };
    (out.status.code() != Some(code) || !body.contains(needle)).then(|| format!("{args:?}: {body}"))
}

#[test]
fn incomplete_coverage_on_any_flag_behaves_as_damaged_on_every_gated_read() {
    let _one = exclusive();
    let scratch = scratch("coverage");
    let mut wrong = Vec::new();
    for (name, flags) in [
        ("discovery", (false, true, true)),
        ("ownership", (true, false, true)),
        ("source", (true, true, false)),
    ] {
        let root = scratch.0.join(name);
        let writer = Store::initialize_fixture(
            &root,
            Limits::default(),
            later(),
            hf_platform::lease::shared(),
        )
        .unwrap();
        let coverage = Coverage {
            discovery_complete: flags.0,
            ownership_complete: flags.1,
            source_complete: flags.2,
        };
        let empty = TaskFixture {
            rows: Vec::new(),
            launches: Vec::new(),
            coverage,
        };
        writer.replace_task_fixture(&empty, later()).unwrap();
        writer
            .add_task_fixture(&task(1, "completed", Some(0), 1, "/r"), later())
            .unwrap();
        drop(writer);
        for (args, out) in gated(&root) {
            wrong.extend(damaged_answer(&args, &out).map(|why| format!("{name} {why}")));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn a_store_with_no_dataset_row_is_state_incompatible_on_every_read() {
    let _one = exclusive();
    let scratch = scratch("nodataset");
    let root = scratch.0.join("store");
    drop(
        Store::initialize_fixture(
            &root,
            Limits::default(),
            later(),
            hf_platform::lease::shared(),
        )
        .unwrap(),
    );
    let mut wrong = Vec::new();
    for (args, out) in gated(&root) {
        let body = text(&out);
        if out.status.code() != Some(1) || !body.contains("\"code\":\"state_incompatible\"") {
            wrong.push(format!("{args:?}: {body}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn rows_are_accounted_at_their_real_size_and_the_batch_continues() {
    let _one = exclusive();
    let scratch = scratch("real-bytes");
    let root = scratch.0.join("store");
    let writer = store(&root);
    let repo = format!("/{}", "r".repeat(30_000));
    for n in 1..=10 {
        writer
            .add_task_fixture(&task(n, "completed", Some(0), 1, &repo), later())
            .unwrap();
    }
    drop(writer);
    let limits = Limits {
        max_rows: 128,
        max_bytes: 100_000,
    };
    let session = ReadSession::open(&root, limits, later(), hf_platform::lease::shared()).unwrap();
    let mut sizes = Vec::new();
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
                after = Some((last.facts.started_at_us, last.facts.id.as_str().to_string()));
                sizes.push(batch.len());
            }
            Ok(())
        })
        .unwrap();
    // A fixed 512 bytes per row would put all ten (300 KB of text) in one batch.
    assert!(sizes.iter().all(|n| *n <= 3), "{sizes:?}");
    assert_eq!(sizes.iter().sum::<usize>(), 10);
}

/// The query budget a single record is measured against: the default `Limits::max_bytes`.
const BUDGET: usize = 1_048_576;

/// A record whose declared size (blobs, text columns and 64 bytes of overhead) is `target`.
fn sized(n: u32, target: usize) -> FixtureTask {
    let mut made = crafted(n, "", "");
    let fixed = made.header.len() + made.status.len() + 64;
    let facts = &made.facts;
    let text = facts.backend.len()
        + facts.session_id.len()
        + facts.status.len()
        + facts.started_at.len()
        + facts.finished_at.as_ref().map_or(0, String::len);
    let repo = target - fixed - text;
    made.facts.repo_path = "r".repeat(repo);
    made
}

/// A known limitation, not a guarantee: a record over the 1 MiB query budget cannot be read
/// by any query, and one such row fails the whole `list`. Python allows 4 MiB per record.
/// This is an E2 exit item; it pins today's behaviour so a change is a decision.
#[test]
fn known_limitation_one_record_over_the_1_mib_query_budget_fails_list_but_not_status_of_others() {
    let _one = exclusive();
    let scratch = scratch("budget");
    let root = scratch.0.join("store");
    let writer = store(&root);
    writer
        .add_task_fixture(&task(1, "completed", Some(0), 1, "/r"), later())
        .unwrap();
    writer.add_task_fixture(&sized(2, BUDGET), later()).unwrap();
    drop(writer);
    // Exactly at the budget the record is readable everywhere, with display bounded on the page.
    for args in [vec!["list"], vec!["status", id(2).as_str()]] {
        let out = ctl(&root, &args);
        assert_eq!(out.status.code(), Some(0), "{args:?}");
        assert!(text(&out).contains(id(2).as_str()));
    }
    let page = ctl(&root, &["task-list-page"]);
    assert_eq!(page.status.code(), Some(0), "{}", &text(&page)[..200]);
    let body = text(&page);
    assert!(body.contains(id(2).as_str()));
    assert!(
        !body.contains(&"r".repeat(201)),
        "page values are display bounded"
    );
    assert!(body.len() < 200_000, "{}", body.len());
    // One byte over: refused as unreadable, never partial or empty.
    let root = scratch.0.join("over");
    let writer = store(&root);
    writer
        .add_task_fixture(&task(1, "completed", Some(0), 1, "/r"), later())
        .unwrap();
    writer
        .add_task_fixture(&sized(2, BUDGET + 1), later())
        .unwrap();
    drop(writer);
    for args in [
        vec!["list"],
        vec!["status", id(2).as_str()],
        vec!["task-list-page"],
    ] {
        let out = ctl(&root, &args);
        let body = text(&out);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{args:?}: {}",
            &body[..body.len().min(300)]
        );
        assert!(body.contains("\"code\":\"state_incompatible\""), "{args:?}");
        assert!(
            body.contains("\"reason\":\"infrastructure_unavailable\""),
            "{args:?}"
        );
        assert!(
            !body.contains("\"tasks\":[") && !body.contains("\"items\":["),
            "{args:?}"
        );
    }
    let healthy = ctl(&root, &["status", id(1).as_str()]);
    assert_eq!(healthy.status.code(), Some(0), "{}", text(&healthy));
}

#[test]
fn a_title_over_1024_bytes_no_longer_breaks_ambiguity_candidates() {
    let _one = exclusive();
    let scratch = scratch("long-title");
    let root = scratch.0.join("store");
    let writer = store(&root);
    for n in 1..=2 {
        let mut made = crafted(n, "", "");
        let shared = TaskId::new(format!("abababab-000{n}-4000-8000-000000000000")).unwrap();
        for blob in [&mut made.header, &mut made.status] {
            let swapped = String::from_utf8(blob.clone()).unwrap();
            *blob = swapped
                .replace(id(n).as_str(), shared.as_str())
                .into_bytes();
        }
        made.facts.id = shared;
        made.facts.title = Some("T".repeat(3000));
        writer.add_task_fixture(&made, later()).unwrap();
    }
    drop(writer);
    let out = ctl(&root, &["status", "abababab"]);
    let body = text(&out);
    assert_eq!(out.status.code(), Some(1), "{body}");
    assert!(body.contains("\"code\":\"ambiguous_task_id\""), "{body}");
    assert!(body.contains(&"T".repeat(200)));
    assert!(!body.contains(&"T".repeat(201)), "candidate titles are cut");
}
