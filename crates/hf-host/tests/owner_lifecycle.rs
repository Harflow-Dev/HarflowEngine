#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[path = "owner_lifecycle/observer.rs"]
mod observer;
#[path = "owner_lifecycle/support.rs"]
mod support;

use hf_application::{
    lease::{LeaseError, LeaseMode},
    process::IdentityPort,
};
use hf_domain::{
    fixture::{Frame, Gate, Stage},
    recovery::CaptureAssessment,
};
use hf_platform::process::MacIdentity;
use std::fs;
use support::{
    assert_effect_for, assert_identity_consistency, assert_terminal_replay, base, deadline,
    inspect, waiting_owner_child,
};

#[test]
fn independent_exclusive_root_claim_is_busy_through_retained_owner_lifecycle() {
    for stage in [
        Stage::OwnerAfterClaim,
        Stage::OwnerAfterChildSpawn,
        Stage::CaptureAfterPublish,
        Stage::OwnerBeforeAck,
    ] {
        let fixture = base::Fixture::new();
        let (_, generation) = fixture.prepare_only(fixture.plan());
        let (listener, socket) = fixture.bind(stage as u8);
        let mut plan = fixture.plan_for(base::request_for(base::OP, base::SESSION, &generation));
        plan.initialize = false;
        plan.gate = Some(Gate {
            stage,
            path: socket.to_string_lossy().into_owned(),
        });
        let mut owner = fixture.direct_owner(plan);
        let mut peer = base::gate_peer(&listener, stage, base::OP, base::NONCE);
        let lock = hf_platform::lease::shared().acquire(
            &fixture.root.join("store.lock"),
            LeaseMode::Exclusive,
            false,
            deadline(1),
        );
        assert!(
            matches!(lock.as_ref().map(|_| ()), Err(LeaseError::Busy)),
            "exclusive root lease at {stage:?}"
        );
        drop(lock);
        base::echo_gate(&mut peer, stage, base::OP, base::NONCE);
        assert!(matches!(base::response(&owner.0), Frame::Accepted { .. }));
        base::finish_protocol(&mut owner);
        assert_eq!(
            fs::read_to_string(&fixture.effects)
                .unwrap()
                .lines()
                .count(),
            1
        );
    }
}

#[test]
fn dispatcher_loss_while_actual_child_is_waiting_preserves_original_identities() {
    let fixture = base::Fixture::new();
    let stage = Stage::OwnerAfterChildSpawn;
    let (listener, socket) = fixture.bind(stage as u8);
    let mut plan = fixture.plan();
    plan.gate = Some(Gate {
        stage,
        path: socket.to_string_lossy().into_owned(),
    });
    let mut parent = fixture.parent(plan);
    let mut peer = base::gate_peer(&listener, stage, base::OP, base::NONCE);
    let before = inspect(&fixture);
    let (owner, child) = waiting_owner_child(&fixture, &before);

    base::kill_direct(&mut parent);
    assert_eq!(
        MacIdentity.identity(owner.pid()),
        Some(owner.clone()),
        "dispatcher death leaves original owner alive"
    );
    assert_eq!(
        MacIdentity.identity(child.pid()),
        Some(child.clone()),
        "dispatcher death leaves original child alive"
    );
    base::echo_gate(&mut peer, stage, base::OP, base::NONCE);
    base::wait_identity_gone(owner.pid());
    base::wait_identity_gone(child.pid());
    let terminal = inspect(&fixture);
    assert_terminal_replay(&fixture, &terminal, &owner, &child);
}

#[test]
fn fresh_readonly_observer_restart_does_not_adopt_or_change_capture_evidence() {
    let fixture = base::Fixture::new();
    let stage = Stage::CaptureAfterPublish;
    let (listener, socket) = fixture.bind(stage as u8);
    let mut plan = fixture.plan();
    plan.gate = Some(Gate {
        stage,
        path: socket.to_string_lossy().into_owned(),
    });
    let mut parent = fixture.parent(plan);
    let mut owner_peer = base::gate_peer(&listener, stage, base::OP, base::NONCE);
    let original = inspect(&fixture);
    support::assert_live_capture(&original);
    assert_identity_consistency(&original, true);
    observer::prove_readonly_restart(&fixture, &original);

    base::kill_direct(&mut parent);
    base::echo_gate(&mut owner_peer, stage, base::OP, base::NONCE);
    base::wait_identity_gone(
        original
            .record
            .ownership
            .owner
            .as_ref()
            .unwrap()
            .process
            .pid(),
    );
    let terminal = inspect(&fixture);
    assert_eq!(
        terminal.capture,
        CaptureAssessment::RecordedTerminalExtentObserved
    );
    let final_capture = terminal.record.capture.as_ref().unwrap();
    assert!(final_capture.progress.stdout_eof && final_capture.progress.stderr_eof);
    assert_eq!(final_capture.exit, hf_domain::process::ProcessExit::Code(0));
    assert_eq!(
        terminal.record.ownership.owner,
        original.record.ownership.owner
    );
    assert_eq!(
        terminal.record.ownership.child,
        original.record.ownership.child
    );
    assert_eq!(
        terminal.record.ownership.request.generation,
        original.record.ownership.request.generation
    );
    assert_eq!(
        terminal.record.events_count,
        original.record.events_count + 1
    );
    assert_effect_for(&fixture, original.record.ownership.child.as_ref().unwrap());
}
