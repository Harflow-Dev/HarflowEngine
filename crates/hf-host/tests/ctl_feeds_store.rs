#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! recovery-activity and statistics-page on roots the fixture port cannot seed: a pending
//! launch and confirmed capture loss for each kind of backend. The expiring statistics budget is
//! covered by ctl_statistics_deadline.rs.
mod ctl_reads_store_support;
use ctl_reads_store_support::*;
use hf_domain::event_read::{CaptureState, LogFacts};
use hf_storage::EventFixture;
use std::path::Path;

const LOST_CLAUDE: &str = "Transcript recovery is not available in this engine";
const UNSUPPORTED: &str = "Backend transcript recovery is unsupported";

fn facts(capture: CaptureState) -> LogFacts {
    LogFacts {
        task_status: Some("running".into()),
        available: true,
        capture,
        output_drained: None,
        eof_finished_seq: None,
        within_grace: false,
    }
}

/// A root with one running task of `backend` whose stream stores `capture`.
fn lost_root(
    scratch: &Scratch,
    name: &str,
    backend: &str,
    capture: CaptureState,
) -> std::path::PathBuf {
    let root = scratch.0.join(name);
    let writer = store(&root);
    let mut one = task(1, "running", None, 1, "/r");
    one.facts.backend = backend.to_string();
    writer.add_task_fixture(&one, later()).unwrap();
    let fixture = EventFixture {
        task: id(1),
        records: Vec::new(),
        partial_tail: 0,
        facts: facts(capture),
    };
    writer.replace_event_fixture(&fixture, later()).unwrap();
    root
}

fn body(output: &std::process::Output) -> String {
    text(output)
}

fn notices(root: &Path) -> Vec<String> {
    let out = ctl(root, &["recovery-activity", id(1).as_str()]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let document = body(&out);
    let start = document.find("\"notices\":[").expect("a notices member") + "\"notices\":[".len();
    let list = &document[start..start + document[start..].find(']').unwrap()];
    assert!(
        document.contains("\"events\":[]") && document.contains("\"display_only\":true"),
        "{document}"
    );
    list.split("\",\"")
        .map(|n| n.trim_matches('"').to_string())
        .collect()
}

#[test]
fn a_pending_launch_is_an_unknown_task_without_data() {
    let _one = exclusive();
    let scratch = scratch("recovery-pending");
    let root = scratch.0.join("store");
    let writer = store(&root);
    writer
        .add_task_fixture(&task(1, "completed", Some(0), 1, "/r"), later())
        .unwrap();
    writer.replace_pending_fixture(&[id(5)], later()).unwrap();
    drop(writer);
    let pending = id(5);
    let out = ctl(&root, &["recovery-activity", pending.as_str()]);
    assert_eq!(out.status.code(), Some(1));
    let error = body(&out);
    assert!(error.contains("\"code\":\"unknown_task\""), "{error}");
    assert!(
        error.contains(&format!(
            "\"message\":\"unknown task_id: {}\"",
            pending.as_str()
        )),
        "{error}"
    );
    assert!(!error.contains("\"data\""), "{error}");
    let status = ctl(&root, &["status", pending.as_str()]);
    assert!(body(&status).contains("\"code\":\"task_pending_launch\""));
}

#[test]
fn confirmed_capture_loss_names_the_rule_of_the_backend() {
    let _one = exclusive();
    let scratch = scratch("recovery-lost");
    let claude = lost_root(&scratch, "claude", "claude", CaptureState::LostWithOwner);
    assert_eq!(notices(&claude), [LOST_CLAUDE]);
    for backend in ["antigravity", "codex", "cursor", "opencode", "vibe"] {
        let root = lost_root(&scratch, backend, backend, CaptureState::LostWithOwner);
        assert_eq!(notices(&root), [UNSUPPORTED], "{backend}");
    }
}

#[test]
fn an_unregistered_backend_with_confirmed_loss_is_invalid_params() {
    let _one = exclusive();
    let scratch = scratch("recovery-unregistered");
    let root = lost_root(&scratch, "nobody", "nobody", CaptureState::LostWithOwner);
    let out = ctl(&root, &["recovery-activity", id(1).as_str()]);
    assert_eq!(out.status.code(), Some(1));
    let error = body(&out);
    assert!(error.contains("\"code\":\"invalid_params\""), "{error}");
    let message = "unknown backend 'nobody'; expected one of ['antigravity', 'claude', 'codex', 'cursor', 'opencode', 'vibe']";
    assert!(
        error.contains(&format!("\"message\":\"{message}\"")),
        "{error}"
    );
    let live = lost_root(&scratch, "nobody-live", "nobody", CaptureState::Live);
    assert_eq!(
        notices(&live),
        ["Transcript recovery requires confirmed capture loss"]
    );
}

#[test]
fn the_other_captures_and_an_observed_outcome_give_the_unconfirmed_notice() {
    let _one = exclusive();
    let scratch = scratch("recovery-others");
    for (name, capture) in [
        ("live", CaptureState::Live),
        ("terminal", CaptureState::ObservedTerminal),
        ("unknown", CaptureState::Unknown),
    ] {
        let root = lost_root(&scratch, name, "claude", capture);
        assert_eq!(
            notices(&root),
            ["Transcript recovery requires confirmed capture loss"],
            "{name}"
        );
    }
}
