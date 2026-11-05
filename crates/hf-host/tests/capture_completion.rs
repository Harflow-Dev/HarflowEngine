#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[path = "capture_completion/support.rs"]
mod support;
use hf_domain::fixture::{Frame, Gate, ReportKind, Stage};
use hf_platform::fixture::{FIXED_STDERR, FIXED_STDOUT};
use std::fs;
use support::*;

#[test]
fn raw_sync_finishes_before_offsets_publish_and_terminal_completion() {
    let fixture = Fixture::new();
    let stage = Stage::CaptureAfterSync;
    let (listener, socket) = fixture.bind(stage as u8);
    let mut plan = fixture.plan();
    plan.gate = Some(Gate {
        stage,
        path: socket.to_string_lossy().into_owned(),
    });
    let mut parent = fixture.parent(plan);
    let mut peer = gate_peer(&listener, stage, OP, NONCE);

    assert_raw_prefix(
        &raw_bytes(&fixture, "stdout"),
        &raw_bytes(&fixture, "stderr"),
    );
    assert_initial_capture(&fixture);
    echo_gate(&mut peer, stage, OP, NONCE);
    assert!(matches!(response(&parent.0), Frame::Accepted { .. }));
    finish(&mut parent);
    assert_raw_fixed(&fixture);
    assert_terminal(
        &fixture,
        FIXED_STDOUT.len() as u64,
        FIXED_STDERR.len() as u64,
    );
    assert_owner_accepted(&fixture);
    assert_eq!(
        fs::read_to_string(&fixture.effects)
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn published_offsets_match_the_already_appended_raw_files() {
    let fixture = Fixture::new();
    let stage = Stage::CaptureAfterPublish;
    let (listener, socket) = fixture.bind(stage as u8);
    let mut plan = fixture.plan();
    plan.gate = Some(Gate {
        stage,
        path: socket.to_string_lossy().into_owned(),
    });
    let mut parent = fixture.parent(plan);
    let mut peer = gate_peer(&listener, stage, OP, NONCE);
    let stdout = raw_bytes(&fixture, "stdout");
    let stderr = raw_bytes(&fixture, "stderr");
    assert_raw_prefix(&stdout, &stderr);
    assert_published_matches_raw(&fixture, &stdout, &stderr);
    echo_gate(&mut peer, stage, OP, NONCE);
    assert!(matches!(response(&parent.0), Frame::Accepted { .. }));
    finish(&mut parent);
    assert_raw_fixed(&fixture);
    assert_terminal(
        &fixture,
        FIXED_STDOUT.len() as u64,
        FIXED_STDERR.len() as u64,
    );
}

#[test]
fn owner_finishes_after_parent_loss_and_replay_does_not_repeat_the_effect() {
    let fixture = Fixture::new();
    let stage = Stage::CaptureBeforeSync;
    let (listener, socket) = fixture.bind(stage as u8);
    let mut plan = fixture.plan();
    plan.gate = Some(Gate {
        stage,
        path: socket.to_string_lossy().into_owned(),
    });
    let mut parent = fixture.parent(plan);
    let mut peer = gate_peer(&listener, stage, OP, NONCE);
    let owner_pid = process_child_pid(
        parent.0.pid(),
        "--internal-owner-fixture-v1",
        &fixture.scratch,
    );

    assert_raw_prefix(
        &raw_bytes(&fixture, "stdout"),
        &raw_bytes(&fixture, "stderr"),
    );
    assert_initial_capture(&fixture);
    kill_direct(&mut parent);
    echo_gate(&mut peer, stage, OP, NONCE);
    wait_identity_gone(owner_pid);
    assert_raw_fixed(&fixture);
    assert_terminal(
        &fixture,
        FIXED_STDOUT.len() as u64,
        FIXED_STDERR.len() as u64,
    );
    assert_eq!(
        fs::read_to_string(&fixture.effects)
            .unwrap()
            .lines()
            .count(),
        1
    );

    let mut replay_plan = fixture.plan_for(request_for(OP, SESSION, &stored_generation(&fixture)));
    replay_plan.initialize = false;
    let mut replay = fixture.parent(replay_plan);
    assert!(matches!(
        response(&replay.0),
        Frame::Report {
            kind: ReportKind::Existing,
            ..
        }
    ));
    finish(&mut replay);
    assert_eq!(
        fs::read_to_string(&fixture.effects)
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_terminal(
        &fixture,
        FIXED_STDOUT.len() as u64,
        FIXED_STDERR.len() as u64,
    );
    assert_owner_accepted(&fixture);
}

#[test]
fn replaced_raw_file_before_terminal_keeps_capture_incomplete_and_owner_reservation() {
    let fixture = Fixture::new();
    let stage = Stage::CaptureBeforeTerminal;
    let (listener, socket) = fixture.bind(stage as u8);
    let mut plan = fixture.plan();
    plan.gate = Some(Gate {
        stage,
        path: socket.to_string_lossy().into_owned(),
    });
    let mut parent = fixture.parent(plan);
    let mut peer = gate_peer(&listener, stage, OP, NONCE);
    let stdout = fixture.scratch.join(format!("r03-b2c.{OP}.stdout"));
    let saved = fixture.scratch.join("saved.stdout");
    fs::rename(&stdout, &saved).unwrap();
    fs::write(&stdout, b"replacement").unwrap();
    echo_gate(&mut peer, stage, OP, NONCE);
    assert!(matches!(
        response(&parent.0),
        Frame::Report {
            kind: ReportKind::Uncertain,
            ..
        }
    ));
    finish(&mut parent);
    assert_incomplete(
        &fixture,
        FIXED_STDOUT.len() as u64,
        FIXED_STDERR.len() as u64,
    );
    assert_owner_accepted(&fixture);
    assert_eq!(
        fs::read_to_string(&fixture.effects)
            .unwrap()
            .lines()
            .count(),
        1
    );
}
