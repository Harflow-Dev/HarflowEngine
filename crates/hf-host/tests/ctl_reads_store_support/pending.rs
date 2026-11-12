//! Launch rows, roots and `harflow-ctl` runs of the pending command tests on real stores.
use super::*;
use hf_storage::{LaunchRow, Limits, Store};
use std::path::{Path, PathBuf};

pub fn launch(n: u32) -> LaunchRow {
    LaunchRow {
        name: id(n).as_str().to_owned(),
        task_id: Some(id(n)),
        generation: Some("c".repeat(32)),
        state: Some("reserved".into()),
        title: Some("Title".into()),
        backend: Some("claude".into()),
        has_resources: Some(false),
        has_worktree: Some(false),
        ..LaunchRow::default()
    }
}

pub fn unqualified(n: u32, code: &str, check: &str) -> LaunchRow {
    LaunchRow {
        name: id(n).as_str().to_owned(),
        task_id: Some(id(n)),
        refusal_code: Some(code.into()),
        refusal_check: Some(check.into()),
        ..LaunchRow::default()
    }
}

pub fn root_of(scratch: &Scratch, label: &str, rows: &[LaunchRow], records: &[u32]) -> PathBuf {
    let root = scratch.0.join(label);
    let writer = store(&root);
    for n in records {
        writer
            .add_task_fixture(&task(*n, "completed", Some(0), 1, "/r"), later())
            .unwrap();
    }
    writer.replace_launch_fixture(rows, later()).unwrap();
    root
}

pub fn reopen(root: &Path) -> Store {
    Store::open(
        root,
        Limits::default(),
        later(),
        hf_platform::lease::shared(),
    )
    .unwrap()
}

pub fn run(root: &Path, args: &[&str]) -> (Option<i32>, String) {
    let out = ctl(root, args);
    (out.status.code(), text(&out))
}

pub fn page(root: &Path, extra: &[&str]) -> (Option<i32>, String) {
    let mut list = vec!["task-pending-page"];
    list.extend_from_slice(extra);
    run(root, &list)
}

pub fn string_of(body: &str, key: &str) -> Option<String> {
    let start = body.find(&format!("\"{key}\":\""))? + key.len() + 4;
    Some(body[start..].split('"').next()?.to_owned())
}

pub fn listed(body: &str) -> Vec<String> {
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

pub fn first_cursor(root: &Path) -> String {
    let (code, body) = page(root, &["--limit", "1"]);
    assert_eq!(code, Some(0), "{body}");
    string_of(&body, "next_cursor").unwrap()
}

pub fn expect_check(root: &Path, extra: &[&str], code: &str, check: &str) {
    let (status, body) = page(root, extra);
    assert_eq!(status, Some(1), "{body}");
    assert!(body.contains(&format!("\"code\":\"{code}\"")), "{body}");
    assert!(body.contains(&format!("\"check\":\"{check}\"")), "{body}");
}

pub fn expect_check_with(result: &(Option<i32>, String), code: &str, check: &str) {
    assert_eq!(result.0, Some(1), "{}", result.1);
    assert!(
        result.1.contains(&format!("\"code\":\"{code}\"")),
        "{}",
        result.1
    );
    assert!(
        result.1.contains(&format!("\"check\":\"{check}\"")),
        "{}",
        result.1
    );
}
