#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! An expired statistics budget through the binary: the scan phase expires a cursor walk and
//! otherwise leaves the page unavailable; it is never a busy store.
mod ctl_reads_store_support;
use ctl_reads_store_support::*;
use std::process::{Command, Output};

const WINDOW: [&str; 4] = [
    "--since",
    "2026-01-01T00:00:00+00:00",
    "--before",
    "2026-02-01T00:00:00+00:00",
];

fn run(root: &std::path::Path, budget_ms: Option<&str>, extra: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_harflow-ctl"));
    command
        .arg("statistics-page")
        .args(WINDOW)
        .args(extra)
        .arg("--json")
        .env("HARFLOW_STATE_ROOT", root)
        .env_remove("HARFLOW_TASK_ID")
        .env_remove("HARFLOW_DEPTH")
        .env_remove("PB_TASK_ID")
        .env_remove("PB_DEPTH");
    if let Some(ms) = budget_ms {
        command.env("HARFLOW_STATISTICS_BUDGET_MS", ms);
    }
    command.output().unwrap()
}

#[test]
fn an_exhausted_budget_is_unavailable_without_a_cursor_and_expired_with_one() {
    let _one = exclusive();
    let scratch = scratch("stats-deadline");
    let root = scratch.0.join("store");
    let writer = store(&root);
    for n in 1..=3 {
        let made = task(n, "completed", Some(0), 1, "/r");
        writer.add_task_fixture(&made, later()).unwrap();
    }
    drop(writer);
    let first = run(&root, None, &["--limit", "1"]);
    let body = text(&first);
    assert_eq!(first.status.code(), Some(0), "{body}");
    let key = "\"next_cursor\":\"";
    let at = body.find(key).unwrap() + key.len();
    let cursor = &body[at..][..body[at..].find('"').unwrap()];

    let plain = run(&root, Some("0"), &[]);
    let body = text(&plain);
    assert_eq!(plain.status.code(), Some(0), "{body}");
    assert!(body.contains("\"state\":\"unavailable\""), "{body}");
    assert!(body.contains("\"code\":\"discovery_budget\""), "{body}");

    let walking = run(&root, Some("0"), &["--cursor", cursor]);
    let body = text(&walking);
    assert_eq!(walking.status.code(), Some(1), "{body}");
    assert!(body.contains("\"code\":\"snapshot_expired\""), "{body}");
    assert!(!body.contains("state_busy"), "{body}");
}
