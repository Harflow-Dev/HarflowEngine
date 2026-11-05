#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[path = "recovery_observation/support.rs"]
mod support;
use hf_domain::{
    fixture::{Gate, Stage},
    recovery::{CaptureAssessment, ProcessState, ReservationDisposition},
};
use std::os::unix::net::UnixStream;
use support::*;

struct GatedOwner {
    peer: UnixStream,
    owner: base::Owned,
    fixture: base::Fixture,
}

#[test]
fn before_sync_tail_is_observational_and_owner_death_keeps_claim() {
    let mut run = gated_owner(Stage::CaptureBeforeSync, 10);
    let identity = owner_identity(&run.owner);
    let report = inspect_stable(&run.fixture);
    assert_eq!(report.owner.as_ref().unwrap().state, ProcessState::Present);
    assert_eq!(report.capture, CaptureAssessment::UnindexedTail);
    assert_eq!(report.reservation, ReservationDisposition::Retained);
    assert!(
        !raw_bytes(&run.fixture, "stdout").is_empty()
            || !raw_bytes(&run.fixture, "stderr").is_empty()
    );
    let oracle = capture_oracle(&run.fixture);
    assert!(
        oracle.starts_with("progress|0,0,"),
        "unpublished raw extent: {oracle}"
    );
    assert_owner_retained(&oracle);

    stop_owner(&mut run);
    let dead = inspect(&run.fixture);
    assert_eq!(dead.owner.as_ref().unwrap().state, ProcessState::AbsentPid);
    assert_eq!(dead.capture, CaptureAssessment::UnindexedTail);
    assert_eq!(dead.reservation, ReservationDisposition::Retained);
    base::wait_identity_gone(identity.pid());
    assert_owner_retained(&capture_oracle(&run.fixture));
    assert_effect_once(&run.fixture);
}

#[test]
fn sync_publish_and_preterminal_cuts_remain_incomplete_observations_after_owner_death() {
    for (number, stage, expected) in [
        (
            11,
            Stage::CaptureAfterSync,
            CaptureAssessment::UnindexedTail,
        ),
        (
            12,
            Stage::CaptureAfterPublish,
            CaptureAssessment::Incomplete,
        ),
        (
            13,
            Stage::CaptureBeforeTerminal,
            CaptureAssessment::Incomplete,
        ),
    ] {
        exercise_dead_cut(number, stage, expected);
    }
}

#[test]
fn terminal_extent_survives_owner_death_without_release_or_effect_replay() {
    let mut run = gated_owner(Stage::CaptureAfterTerminal, 14);
    let report = inspect_stable(&run.fixture);
    assert_eq!(
        report.capture,
        CaptureAssessment::RecordedTerminalExtentObserved
    );
    assert_eq!(report.reservation, ReservationDisposition::Retained);
    assert_owner_retained(&capture_oracle(&run.fixture));
    assert_effect_once(&run.fixture);

    stop_owner(&mut run);
    let dead = inspect(&run.fixture);
    assert_eq!(
        dead.capture,
        CaptureAssessment::RecordedTerminalExtentObserved
    );
    assert_eq!(dead.owner.as_ref().unwrap().state, ProcessState::AbsentPid);
    assert_eq!(dead.reservation, ReservationDisposition::Retained);
    assert_effect_once(&run.fixture);
}

#[test]
fn parent_ack_loss_after_published_cut_keeps_terminal_evidence_and_no_replay() {
    let fixture = base::Fixture::new();
    let stage = Stage::CaptureBeforeSync;
    let (listener, socket) = fixture.bind(15);
    let mut plan = fixture.plan();
    plan.gate = Some(Gate {
        stage,
        path: socket.to_string_lossy().into_owned(),
    });
    let mut parent = fixture.parent(plan);
    let mut peer = base::gate_peer(&listener, stage, OP, NONCE);
    let before = inspect_stable(&fixture);
    let owner_pid = before
        .record
        .ownership
        .owner
        .as_ref()
        .expect("original durable owner")
        .process
        .pid();
    assert_eq!(before.capture, CaptureAssessment::UnindexedTail);
    assert_eq!(before.reservation, ReservationDisposition::Retained);

    base::kill_direct(&mut parent);
    base::echo_gate(&mut peer, stage, OP, NONCE);
    base::wait_identity_gone(owner_pid);
    let after = inspect_stable(&fixture);
    assert_eq!(after.owner.as_ref().unwrap().state, ProcessState::AbsentPid);
    assert_eq!(
        after.capture,
        CaptureAssessment::RecordedTerminalExtentObserved
    );
    assert_eq!(after.reservation, ReservationDisposition::Retained);
    let oracle = capture_oracle(&fixture);
    assert!(
        oracle.starts_with("terminal|"),
        "capture completes despite lost parent acknowledgment: {oracle}"
    );
    assert_owner_retained(&oracle);
    assert_effect_once(&fixture);
}

fn gated_owner(stage: Stage, gate_number: u8) -> GatedOwner {
    let fixture = base::Fixture::new();
    let (_, generation) = fixture.prepare_only(fixture.plan());
    let (listener, socket) = fixture.bind(gate_number);
    let mut plan = fixture.plan_for(base::request_for(OP, SESSION, &generation));
    plan.initialize = false;
    plan.gate = Some(Gate {
        stage,
        path: socket.to_string_lossy().into_owned(),
    });
    let owner = fixture.direct_owner(plan);
    let peer = base::gate_peer(&listener, stage, OP, NONCE);
    GatedOwner {
        peer,
        owner,
        fixture,
    }
}

fn exercise_dead_cut(number: u8, stage: Stage, expected: CaptureAssessment) {
    let mut run = gated_owner(stage, number);
    let report = inspect_stable(&run.fixture);
    assert_eq!(report.owner.as_ref().unwrap().state, ProcessState::Present);
    assert_eq!(report.capture, expected, "classification at {stage:?}");
    assert_eq!(report.reservation, ReservationDisposition::Retained);
    let oracle = capture_oracle(&run.fixture);
    assert_owner_retained(&oracle);

    stop_owner(&mut run);
    let dead = inspect(&run.fixture);
    assert_eq!(dead.owner.as_ref().unwrap().state, ProcessState::AbsentPid);
    assert_eq!(dead.capture, expected);
    assert_eq!(dead.reservation, ReservationDisposition::Retained);
    assert_eq!(
        capture_oracle(&run.fixture),
        oracle,
        "owner death did not change {stage:?} evidence"
    );
    assert_effect_once(&run.fixture);
}

fn inspect_stable(fixture: &base::Fixture) -> hf_domain::recovery::RecoveryReport {
    let before = file_state(fixture);
    let report = inspect(fixture);
    let after = file_state(fixture);
    assert_eq!(
        before, after,
        "inspection preserves listed source and raw bytes"
    );
    report
}

fn stop_owner(run: &mut GatedOwner) {
    base::kill_direct(&mut run.owner);
    let _ = run.peer.shutdown(std::net::Shutdown::Both);
}

fn owner_identity(owner: &base::Owned) -> hf_domain::process::ProcessIdentity {
    owner
        .0
        .registered_identity()
        .expect("retained owner identity")
        .clone()
}

fn assert_effect_once(fixture: &base::Fixture) {
    assert_eq!(
        std::fs::read_to_string(&fixture.effects)
            .unwrap()
            .lines()
            .count(),
        1
    );
}

fn assert_owner_retained(value: &str) {
    let owner = value.lines().nth(1).expect("independent owner row");
    assert!(
        owner.starts_with("owner:3:0:none:"),
        "reservation remains active: {value}"
    );
}
