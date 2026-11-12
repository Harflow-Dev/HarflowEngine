#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The pending launch commands on real roots: the order of refusals, the exact-id status,
//! and the ordinary reads of a receipt that is no launch document.
mod ctl_reads_store_support;
use ctl_reads_store_support::{pending::*, *};
use std::path::PathBuf;

#[test]
fn a_marker_makes_an_agent_caller_before_the_root_is_looked_at() {
    let _one = exclusive();
    let scratch = scratch("markers");
    let root = root_of(&scratch, "r", &[launch(1)], &[]);
    let missing = scratch.0.join("missing");
    for marker in ["HARFLOW_TASK_ID", "HARFLOW_DEPTH", "PB_TASK_ID", "PB_DEPTH"] {
        for value in ["x", ""] {
            for target in [&root, &missing] {
                let out = ctl_env(target, &["task-pending-page"], &[(marker, value)]);
                let body = text(&out);
                assert_eq!(out.status.code(), Some(1), "{marker}={value}: {body}");
                assert!(body.contains("\"code\":\"agent_caller\""), "{body}");
                assert!(
                    body.contains(
                        "\"data\":{\"check\":\"direct_caller_required\",\"coverage\":\"unknown\"}"
                    ),
                    "{body}"
                );
                assert!(!body.contains("task_id\":\""), "{body}");
            }
        }
    }
    let out = ctl_env(
        &root,
        &["task-pending-status", "--task-id", "x"],
        &[("PB_DEPTH", "")],
    );
    assert!(text(&out).contains("\"code\":\"agent_caller\""));
}

#[test]
fn an_unreadable_root_is_unavailable_and_is_never_created() {
    let _one = exclusive();
    let scratch = scratch("unavailable");
    let missing = scratch.0.join("missing");
    let empty = scratch.0.join("empty");
    std::fs::create_dir(&empty).unwrap();
    for root in [&missing, &empty] {
        for args in [
            vec!["task-pending-page"],
            vec!["task-pending-status", "--task-id", "x"],
        ] {
            let (code, body) = run(root, &args);
            assert_eq!(code, Some(1), "{body}");
            assert!(
                body.contains("\"code\":\"pending_observation_unavailable\""),
                "{body}"
            );
            assert!(
                body.contains("\"data\":{\"coverage\":\"unknown\"}"),
                "{body}"
            );
            assert!(!body.contains("\"check\""), "{body}");
        }
    }
    assert!(!missing.exists(), "the root is never initialized");
    assert_eq!(std::fs::read_dir(&empty).unwrap().count(), 0);
}

#[test]
fn the_status_finds_an_id_only_by_its_exact_full_form() {
    let _one = exclusive();
    let scratch = scratch("status");
    let root = root_of(&scratch, "r", &[launch(5)], &[]);
    let full = id(5);
    let status = |task: &str| run(&root, &["task-pending-status", "--task-id", task]);
    let (code, body) = status(full.as_str());
    assert_eq!(code, Some(0), "{body}");
    assert!(body.contains("\"disposition\":\"pending\""), "{body}");
    assert!(body.contains("\"state\":\"reserved\""), "{body}");
    for prefix in [&full.as_str()[..8], &full.as_str()[..35]] {
        let (code, body) = status(prefix);
        assert_eq!(code, Some(0), "{body}");
        assert!(
            body.contains("\"disposition\":\"not_found\""),
            "{prefix}: {body}"
        );
    }
    let (code, body) = status("not an id!");
    assert_eq!(code, Some(1), "{body}");
    assert!(body.contains("\"code\":\"invalid_task_id\""), "{body}");
    assert!(
        body.contains("\"check\":\"exact_task_id_required\""),
        "{body}"
    );
}

#[test]
fn a_record_that_shares_the_id_wins_and_is_filtered_from_pages() {
    let _one = exclusive();
    let scratch = scratch("recorded");
    let root = root_of(&scratch, "r", &[launch(1), launch(2)], &[1, 3]);
    let status = |n: u32| run(&root, &["task-pending-status", "--task-id", id(n).as_str()]).1;
    assert!(
        status(1).contains("\"disposition\":\"recorded\""),
        "launch and record"
    );
    assert!(
        status(3).contains("\"disposition\":\"recorded\""),
        "a bare record"
    );
    assert!(status(2).contains("\"disposition\":\"pending\""));
    assert!(status(4).contains("\"disposition\":\"not_found\""));
    let (code, body) = page(&root, &[]);
    assert_eq!(code, Some(0), "{body}");
    assert_eq!(listed(&body), [id(2).as_str()]);
    assert!(body.contains("\"examined_count\":2"), "{body}");
    assert!(body.contains("\"coverage\":\"complete\""), "{body}");
}

#[test]
fn a_filtered_row_moves_the_cursor_forward_without_listing() {
    let _one = exclusive();
    let scratch = scratch("filtered");
    let root = root_of(&scratch, "r", &[launch(1), launch(2), launch(3)], &[1, 2]);
    let (code, body) = page(&root, &["--limit", "1"]);
    assert_eq!(code, Some(0), "{body}");
    assert_eq!(listed(&body), [id(3).as_str()]);
    assert!(body.contains("\"examined_count\":3"), "{body}");
    assert!(body.contains("\"has_more\":false"), "{body}");
}

fn spawned_root(scratch: &Scratch) -> PathBuf {
    let mut spawned = launch(1);
    spawned.state = Some("spawned".into());
    root_of(
        scratch,
        "r",
        &[spawned, unqualified(2, "format", "format")],
        &[],
    )
}

#[test]
fn spawned_receipts_without_a_record_are_unknown_and_unlisted() {
    let _one = exclusive();
    let scratch = scratch("spawned");
    let root = spawned_root(&scratch);
    let (_, body) = run(&root, &["task-pending-status", "--task-id", id(1).as_str()]);
    assert!(body.contains("\"disposition\":\"unknown\""), "{body}");
    assert!(
        body.contains("\"code\":\"pending_record_inconsistent\""),
        "{body}"
    );
    assert!(
        body.contains("\"check\":\"postrecord_source_without_record\""),
        "{body}"
    );
    let (_, body) = page(&root, &[]);
    assert_eq!(listed(&body), [id(2).as_str()], "{body}");
    assert!(body.contains("\"disposition\":\"refused\""), "{body}");
    assert!(
        body.contains("\"coverage\":\"unknown\""),
        "a refused row is uncertainty: {body}"
    );
}

#[test]
fn ordinary_reads_report_the_document_error_of_an_unqualified_receipt() {
    let _one = exclusive();
    let scratch = scratch("ordinary");
    let root = root_of(
        &scratch,
        "r",
        &[
            unqualified(1, "format", "format"),
            unqualified(2, "oversize", "oversize"),
        ],
        &[],
    );
    let prefix = format!("{}/tasks/", root.display());
    let (code, body) = run(&root, &["status", id(1).as_str()]);
    assert_eq!(code, Some(1), "{body}");
    assert!(body.contains("\"code\":\"format\""), "{body}");
    let message = format!(
        "{prefix}{}.launch.json [format]: Incompatible task launch format",
        id(1).as_str()
    );
    assert!(
        body.contains(&format!("\"message\":\"{message}\"")),
        "{body}"
    );
    assert!(!body.contains("\"data\""), "{body}");
    assert!(!body.contains("unknown_task"), "{body}");

    let (code, body) = run(&root, &["task-events", id(2).as_str()]);
    assert_eq!(code, Some(1), "{body}");
    assert!(body.contains("\"code\":\"oversize\""), "{body}");
    assert!(
        body.contains("[oversize]: Task launch exceeds 65536 bytes"),
        "{body}"
    );
    assert!(!body.contains("\"data\""), "{body}");

    let ids = format!("{},{}", id(1).as_str(), id(2).as_str());
    let (code, body) = run(&root, &["status-batch", "--task-ids", &ids]);
    assert_eq!(code, Some(0), "{body}");
    assert!(
        body.contains("\"code\":\"format\"") && body.contains("\"code\":\"oversize\""),
        "{body}"
    );
    assert!(
        !body.contains("unknown_task") && !body.contains("\"data\""),
        "{body}"
    );
}

#[test]
fn a_resource_name_refusal_still_reads_as_a_pending_launch_in_ordinary_reads() {
    let _one = exclusive();
    let scratch = scratch("resource-names");
    let mut row = launch(1);
    row.title = None;
    row.backend = None;
    row.generation = None;
    row.has_resources = Some(true);
    row.refusal_code = Some("field_shape".into());
    row.refusal_check = Some("resource_names".into());
    let root = root_of(&scratch, "r", &[row], &[]);
    let (_, body) = run(&root, &["status", id(1).as_str()]);
    assert!(body.contains("\"code\":\"task_pending_launch\""), "{body}");
    let (_, body) = run(&root, &["task-pending-status", "--task-id", id(1).as_str()]);
    assert!(body.contains("\"code\":\"field_shape\""), "{body}");
    assert!(body.contains("\"check\":\"resource_names\""), "{body}");
}

#[test]
fn text_is_cut_on_a_character_boundary_and_the_cut_is_reported() {
    let _one = exclusive();
    let scratch = scratch("cut");
    let mut row = launch(1);
    row.title = Some("\u{e9}".repeat(600));
    let root = root_of(&scratch, "r", &[row], &[]);
    let (_, body) = run(&root, &["task-pending-status", "--task-id", id(1).as_str()]);
    assert!(body.contains("\"metadata_truncated\":true"), "{body}");
    let title = string_of(&body, "title").unwrap();
    // The transport writes non-ASCII as escapes: 512 characters are the first 1,024 bytes.
    assert_eq!(title, "\\u00e9".repeat(512));
}
