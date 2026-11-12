#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `task-pending-page` on real roots: the page walk, the namespace and read bounds reached
//! through the debug-only limit seam, the response bound, and the life of a cursor.
mod ctl_reads_store_support;
use ctl_reads_store_support::*;
use hf_domain::task_read::TaskId;
use hf_storage::LaunchRow;
use std::path::{Path, PathBuf};

fn name(n: u32) -> TaskId {
    id(n)
}

/// A launch with no resources: 36 + 8 + 32 + 6 = 82 stored bytes.
fn plain(n: u32) -> LaunchRow {
    LaunchRow {
        name: name(n).as_str().to_owned(),
        task_id: Some(name(n)),
        generation: Some("c".repeat(32)),
        state: Some("reserved".into()),
        backend: Some("claude".into()),
        has_resources: Some(false),
        has_worktree: Some(false),
        ..LaunchRow::default()
    }
}
/// The reference charges this for every receipt it reads.
const ROW_BYTES: usize = 65_537;

fn invalid(text: &str) -> LaunchRow {
    LaunchRow {
        name: text.to_owned(),
        ..LaunchRow::default()
    }
}

fn root_with(scratch: &Scratch, label: &str, rows: &[LaunchRow], tasks: u32) -> PathBuf {
    damaged_root(scratch, label, rows, tasks, &[])
}

/// A root whose records `damaged` (by number) are damaged on top of `tasks` intact ones.
fn damaged_root(
    scratch: &Scratch,
    label: &str,
    rows: &[LaunchRow],
    tasks: u32,
    damaged: &[u32],
) -> PathBuf {
    let root = scratch.0.join(label);
    let writer = store(&root);
    for n in 1..=tasks {
        writer
            .add_task_fixture(&task(100 + n, "completed", Some(0), 1, "/r"), later())
            .unwrap();
    }
    for n in damaged {
        writer.damage_task_fixture(&id(900 + n), later()).unwrap();
    }
    writer.replace_launch_fixture(rows, later()).unwrap();
    root
}

fn page(root: &Path, extra: &[&str], env: &[(&str, &str)]) -> (Option<i32>, String) {
    let mut list = vec!["task-pending-page"];
    list.extend_from_slice(extra);
    let out = ctl_env(root, &list, env);
    (out.status.code(), text(&out))
}

/// The raw text of a string member, read from compact JSON.
fn string_of(body: &str, key: &str) -> Option<String> {
    let start = body.find(&format!("\"{key}\":\""))? + key.len() + 4;
    Some(body[start..].split('"').next()?.to_owned())
}

fn listed(body: &str) -> Vec<String> {
    body.match_indices("\"task_id\":\"")
        .map(|(at, found)| {
            body[at + found.len()..]
                .split('"')
                .next()
                .unwrap()
                .to_owned()
        })
        .collect()
}

#[test]
fn a_page_lists_launches_in_order_with_the_host_clock() {
    let _one = exclusive();
    let scratch = scratch("page");
    let rows = [plain(3), plain(1), plain(2)];
    let root = root_with(&scratch, "r", &rows, 0);
    let (code, body) = page(&root, &[], &[]);
    assert_eq!(code, Some(0), "{body}");
    assert_eq!(
        listed(&body),
        [name(1), name(2), name(3)].map(|id| id.as_str().to_owned())
    );
    assert!(body.contains("\"coverage\":\"complete\""), "{body}");
    assert!(body.contains("\"examined_count\":3"), "{body}");
    assert!(body.contains("\"has_more\":false,"), "{body}");
    assert!(body.contains("\"next_cursor\":null"), "{body}");
    assert!(body.contains("\"bound\":null"), "{body}");
    assert!(body.contains("\"invalid_name_count\":0"), "{body}");
    let observed = string_of(&body, "observed_at").unwrap();
    assert!(
        observed.starts_with("20") && observed.ends_with("+00:00"),
        "{observed}"
    );
}

#[test]
fn prefix_nested_ids_are_not_skipped_at_limit_one() {
    let _one = exclusive();
    let scratch = scratch("nested");
    let (short, long) = ("ab", "ab-c");
    let rows: Vec<LaunchRow> = [short, long]
        .iter()
        .map(|text| LaunchRow {
            name: (*text).to_owned(),
            task_id: Some(TaskId::new((*text).to_owned()).unwrap()),
            ..plain(1)
        })
        .collect();
    let root = root_with(&scratch, "r", &rows, 0);
    let (_, first) = page(&root, &["--limit", "1"], &[]);
    assert_eq!(listed(&first), [long], "{first}");
    let cursor = string_of(&first, "next_cursor").unwrap();
    let (code, second) = page(&root, &["--limit", "1", "--cursor", &cursor], &[]);
    assert_eq!(code, Some(0), "{second}");
    assert_eq!(listed(&second), [short], "{second}");
    assert!(second.contains("\"has_more\":false"), "{second}");
}

#[test]
fn the_read_budget_cuts_a_page_after_the_rows_it_paid_for() {
    let _one = exclusive();
    let scratch = scratch("budget");
    let root = root_with(&scratch, "r", &[plain(1), plain(2), plain(3)], 0);
    let two = (2 * ROW_BYTES).to_string();
    let (code, body) = page(&root, &[], &[("HARFLOW_PENDING_READ_BYTES", &two)]);
    assert_eq!(code, Some(0), "{body}");
    assert_eq!(listed(&body).len(), 2, "{body}");
    assert!(body.contains("\"bound\":\"read_budget\""), "{body}");
    assert!(body.contains("\"coverage\":\"unknown\""), "{body}");
    assert!(body.contains("\"has_more\":true"), "{body}");
    assert!(body.contains("\"examined_count\":2"), "{body}");
    let all = (3 * ROW_BYTES).to_string();
    let (_, full) = page(&root, &[], &[("HARFLOW_PENDING_READ_BYTES", &all)]);
    assert_eq!(listed(&full).len(), 3, "{full}");
    assert!(full.contains("\"bound\":null"), "{full}");
}

#[test]
fn a_read_budget_spent_before_any_row_is_an_error() {
    let _one = exclusive();
    let scratch = scratch("budget-zero");
    let root = root_with(&scratch, "r", &[plain(1)], 0);
    let few = (ROW_BYTES - 1).to_string();
    let (code, body) = page(&root, &[], &[("HARFLOW_PENDING_READ_BYTES", &few)]);
    assert_eq!(code, Some(1), "{body}");
    assert!(
        body.contains("\"code\":\"pending_observation_bound\""),
        "{body}"
    );
    assert!(
        body.contains("\"data\":{\"check\":\"read_budget\",\"coverage\":\"unknown\"}"),
        "{body}"
    );
    assert!(!body.contains("task_id"), "{body}");
}

#[test]
fn an_exhausted_candidate_check_makes_the_caller_undecidable_and_discloses_nothing() {
    let _one = exclusive();
    let scratch = scratch("candidates");
    let mut row = plain(1);
    row.title = Some("SECRET-TITLE".into());
    let root = root_with(&scratch, "r", &[row], 1);
    // One record: its header (62 bytes) and status (50 bytes), charged by raw length.
    for (bytes, undecided) in [("111", true), ("112", false)] {
        let (code, body) = page(&root, &[], &[("HARFLOW_PENDING_READ_BYTES", bytes)]);
        if undecided {
            assert_eq!(code, Some(1), "{body}");
            assert!(body.contains("\"code\":\"caller_undecidable\""), "{body}");
            assert!(
                body.contains(
                    "\"data\":{\"check\":\"direct_caller_required\",\"coverage\":\"unknown\"}"
                ),
                "{body}"
            );
            assert!(
                !body.contains("SECRET-TITLE") && !body.contains("task_id\":\""),
                "{body}"
            );
        } else {
            // The check passes; the row itself then no longer fits.
            assert!(body.contains("\"read_budget\""), "{body}");
        }
    }
}

#[test]
fn a_damaged_unrelated_record_makes_the_caller_undecidable() {
    let _one = exclusive();
    let scratch = scratch("damaged");
    let intact = root_with(&scratch, "intact", &[plain(1)], 1);
    let (code, body) = page(&intact, &[], &[]);
    assert_eq!(code, Some(0), "an intact history is no obstacle: {body}");
    let broken = damaged_root(&scratch, "broken", &[plain(1)], 1, &[1]);
    let (code, body) = page(&broken, &[], &[]);
    assert_eq!(code, Some(1), "{body}");
    assert!(body.contains("\"code\":\"caller_undecidable\""), "{body}");
    assert!(!body.contains("task_id\":\""), "{body}");
}

#[test]
fn the_namespace_entry_bound_is_inclusive() {
    let _one = exclusive();
    let scratch = scratch("entries");
    let root = root_with(&scratch, "r", &[plain(1), plain(2)], 0);
    let (code, body) = page(&root, &[], &[("HARFLOW_PENDING_ENTRIES", "2")]);
    assert_eq!(code, Some(0), "{body}");
    // The caller check meets the bound first, as in the reference: nothing is disclosed.
    let (code, body) = page(&root, &[], &[("HARFLOW_PENDING_ENTRIES", "1")]);
    assert_eq!(code, Some(1), "{body}");
    assert!(body.contains("\"code\":\"caller_undecidable\""), "{body}");
    assert!(!body.contains("task_id\":\""), "{body}");
}

#[test]
fn the_namespace_name_byte_bound_counts_the_suffix_and_is_inclusive() {
    let _one = exclusive();
    let scratch = scratch("name-bytes");
    let root = root_with(&scratch, "r", &[plain(1), invalid("bad name")], 0);
    // 36 + 8 name bytes plus two ".launch.json" suffixes.
    let exact = (36 + 8 + 24).to_string();
    let (code, body) = page(&root, &[], &[("HARFLOW_PENDING_NAME_BYTES", &exact)]);
    assert_eq!(code, Some(0), "{body}");
    let below = (36 + 8 + 23).to_string();
    let (code, body) = page(&root, &[], &[("HARFLOW_PENDING_NAME_BYTES", &below)]);
    assert_eq!(code, Some(1), "{body}");
    assert!(body.contains("\"code\":\"caller_undecidable\""), "{body}");
}

#[test]
fn limit_overrides_that_are_not_numbers_are_ignored() {
    let _one = exclusive();
    let scratch = scratch("junk");
    let root = root_with(&scratch, "r", &[plain(1)], 0);
    let junk = [
        ("HARFLOW_PENDING_ENTRIES", "many"),
        ("HARFLOW_PENDING_NAME_BYTES", "-1"),
        ("HARFLOW_PENDING_READ_BYTES", ""),
    ];
    let (code, body) = page(&root, &[], &junk);
    assert_eq!(code, Some(0), "{body}");
    assert_eq!(listed(&body).len(), 1);
}

#[test]
fn a_page_that_outgrows_the_response_bound_is_partial_and_the_walk_continues() {
    let _one = exclusive();
    let scratch = scratch("response");
    let rows: Vec<LaunchRow> = (1..=20)
        .map(|n| LaunchRow {
            title: Some("t".repeat(1024)),
            repo_path: Some("p".repeat(4096)),
            created_at: Some("c".repeat(1024)),
            not_started_reason: Some("n".repeat(1024)),
            ..plain(n)
        })
        .collect();
    let root = root_with(&scratch, "r", &rows, 0);
    let (code, first) = page(&root, &["--limit", "20"], &[]);
    assert_eq!(code, Some(0), "{first}");
    assert!(first.contains("\"bound\":\"response_bytes\""), "{first}");
    assert!(first.contains("\"coverage\":\"unknown\""), "{first}");
    assert!(first.contains("\"has_more\":true"), "{first}");
    assert!(first.len() < 128 * 1024 - 1024, "{}", first.len());
    let mut seen = listed(&first);
    assert!(seen.len() > 5 && seen.len() < 20, "{}", seen.len());
    let mut cursor = string_of(&first, "next_cursor").unwrap();
    for _ in 0..10 {
        let (code, next) = page(&root, &["--limit", "20", "--cursor", &cursor], &[]);
        assert_eq!(code, Some(0), "{next}");
        seen.extend(listed(&next));
        match string_of(&next, "next_cursor") {
            Some(more) => cursor = more,
            None => break,
        }
    }
    let wanted: Vec<String> = (1..=20).map(|n| name(n).as_str().to_owned()).collect();
    assert_eq!(seen, wanted);
}

#[test]
fn an_invalid_name_makes_the_walk_unknown_from_the_first_page_to_the_last() {
    let _one = exclusive();
    let scratch = scratch("invalid-name");
    let root = root_with(
        &scratch,
        "r",
        &[plain(1), plain(2), plain(3), invalid("bad name")],
        0,
    );
    let (_, first) = page(&root, &["--limit", "1"], &[]);
    assert!(first.contains("\"invalid_name_count\":1"), "{first}");
    assert!(first.contains("\"coverage\":\"unknown\""), "{first}");
    let mut cursor = string_of(&first, "next_cursor").unwrap();
    let last = loop {
        let (code, next) = page(&root, &["--limit", "1", "--cursor", &cursor], &[]);
        assert_eq!(code, Some(0), "{next}");
        assert!(next.contains("\"coverage\":\"unknown\""), "{next}");
        match string_of(&next, "next_cursor") {
            Some(more) => cursor = more,
            None => break next,
        }
    };
    assert!(last.contains("\"has_more\":false"), "{last}");
}
