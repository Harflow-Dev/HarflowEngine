#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `harflow-ctl` on a root whose verification fence `store.fence` is present: the
//! read gate's state inspection refuses every read before a session opens, and
//! state-inspect reports the root as incompatible. The session's own fence message
//! (`ReadRefusal::Fenced`) is reached only when a fence lands between gate and read.
mod ctl_reads_store_support;
use ctl_reads_store_support::*;
use std::{fs, path::Path};

const FENCED: &str = "store verification failed; the history cannot be trusted";
const GATED: &str = "State root cannot be read";

fn generation(root: &Path) -> String {
    let marker = fs::read_to_string(root.join("store.identity")).unwrap();
    marker.lines().nth(2).unwrap().to_owned()
}

fn fence(root: &Path, generation: &str) {
    fs::write(
        root.join("store.fence"),
        format!("harflow-fence-1\n{generation}\nfailed\n"),
    )
    .unwrap();
}

/// A root with one completed task, no writer left open.
fn seeded(root: &Path) {
    let writer = store(root);
    writer
        .add_task_fixture(&task(1, "completed", Some(0), 1, "/r"), later())
        .unwrap();
    drop(writer);
}

fn reads() -> Vec<Vec<String>> {
    let one = id(1).as_str().to_string();
    [
        vec!["list"],
        vec!["status", &one],
        vec!["status-batch", "--task-ids", &one],
        vec!["task-list-page"],
    ]
    .into_iter()
    .map(|args| args.into_iter().map(str::to_string).collect())
    .collect()
}

fn run(root: &Path, args: &[String]) -> (Option<i32>, String) {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = ctl(root, &args);
    (out.status.code(), text(&out))
}

fn verdict(root: &Path) -> String {
    let out = ctl(root, &["state-inspect"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    text(&out)
}

#[test]
fn a_fenced_root_refuses_every_read_as_unavailable() {
    let _one = exclusive();
    let scratch = scratch("fence-reads");
    let root = scratch.0.join("store");
    seeded(&root);
    for args in reads() {
        assert_eq!(run(&root, &args).0, Some(0), "{args:?}: baseline");
    }
    fence(&root, &generation(&root));
    let before = fs::read(root.join("store.fence")).unwrap();
    for args in reads() {
        let (code, body) = run(&root, &args);
        assert_eq!(code, Some(1), "{args:?}: {body}");
        assert!(
            body.contains("\"code\":\"state_incompatible\""),
            "{args:?}: {body}"
        );
        assert!(
            body.contains("\"reason\":\"infrastructure_unavailable\""),
            "{args:?}: {body}"
        );
        assert!(body.contains(GATED), "{args:?}: {body}");
        assert!(!body.contains(id(1).as_str()), "{args:?}: no rows leak");
    }
    assert_eq!(fs::read(root.join("store.fence")).unwrap(), before);
}

#[test]
fn a_foreign_generation_fence_refuses_reads_as_an_invalid_marker() {
    let _one = exclusive();
    let scratch = scratch("fence-foreign");
    let root = scratch.0.join("store");
    seeded(&root);
    fence(&root, &"f".repeat(32));
    for args in reads() {
        let (code, body) = run(&root, &args);
        assert_eq!(code, Some(1), "{args:?}: {body}");
        assert!(
            body.contains("\"code\":\"state_incompatible\""),
            "{args:?}: {body}"
        );
        assert!(
            body.contains("\"reason\":\"invalid_marker\""),
            "{args:?}: {body}"
        );
        assert!(!body.contains(FENCED), "{args:?}: {body}");
    }
}

#[test]
fn state_inspect_reports_fence_verdicts_without_changing_the_root() {
    let _one = exclusive();
    let scratch = scratch("fence-inspect");
    let root = scratch.0.join("store");
    seeded(&root);
    let ready = verdict(&root);
    assert!(ready.contains("\"status\":\"ready\""), "{ready}");
    assert!(
        !root.join("store.fence").exists(),
        "inspection creates no fence"
    );
    let current = generation(&root);
    let cases = [
        (
            "current generation",
            format!("harflow-fence-1\n{current}\nfailed\n"),
            "infrastructure_unavailable",
        ),
        (
            "foreign generation",
            format!("harflow-fence-1\n{}\nfailed\n", "f".repeat(32)),
            "invalid_marker",
        ),
        (
            "unknown state",
            format!("harflow-fence-1\n{current}\npassed\n"),
            "invalid_marker",
        ),
        ("empty", String::new(), "invalid_marker"),
        (
            "oversized",
            format!("harflow-fence-1\n{current}\nfailed\n{}", "x".repeat(200)),
            "invalid_marker",
        ),
    ];
    for (label, content, reason) in cases {
        fs::write(root.join("store.fence"), &content).unwrap();
        let body = verdict(&root);
        assert!(
            body.contains("\"status\":\"incompatible\""),
            "{label}: {body}"
        );
        assert!(
            body.contains(&format!("\"reason\":\"{reason}\"")),
            "{label}: {body}"
        );
        assert_eq!(
            fs::read_to_string(root.join("store.fence")).unwrap(),
            content
        );
    }
    fs::remove_file(root.join("store.fence")).unwrap();
    fs::create_dir(root.join("store.fence")).unwrap();
    let body = verdict(&root);
    assert!(
        body.contains("\"reason\":\"invalid_marker\""),
        "directory: {body}"
    );
}
