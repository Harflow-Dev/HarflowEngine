#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[path = "handoff_losses/continuity.rs"]
mod continuity;
#[path = "handoff_losses/support.rs"]
mod support;

use hf_domain::fixture::{Frame, Gate, ReportKind, Stage};
use std::fs;
use support::*;

#[test]
fn parent_loss_at_preparation_checkpoints_never_releases_or_launches() {
    for (stage, expected) in [
        (Stage::ParentBeforePrepare, None),
        (Stage::ParentAfterPrepare, Some(0)),
        (Stage::ParentBeforeSpawn, Some(0)),
        (Stage::ParentAfterOwnerSpawn, Some(0)),
    ] {
        let fixture = Fixture::new();
        let (listener, socket) = fixture.bind(stage as u8);
        let mut plan = fixture.plan();
        plan.gate = Some(Gate {
            stage,
            path: socket.to_string_lossy().into_owned(),
        });
        let mut parent = fixture.parent(plan);
        let peer = gate_peer(&listener, stage, OP, NONCE);
        let owner_pid = if stage == Stage::ParentAfterOwnerSpawn {
            Some(process_child_pid(
                parent.0.pid(),
                "--internal-owner-fixture-v1",
                &fixture.scratch,
            ))
        } else {
            None
        };
        kill_direct(&mut parent);
        drop(peer);
        if let Some(pid) = owner_pid {
            wait_identity_gone(pid);
        }
        let raw = oracle(&fixture.root, OP, &fixture.scratch);
        if let Some(phase) = expected {
            assert_eq!(number(&raw, "phase"), Some(phase), "{raw}");
        } else {
            assert_eq!(raw, "null");
        }
        assert_empty(&fixture.effects);
    }
}

#[test]
fn simultaneous_same_session_requests_reserve_at_most_one_effect_right() {
    let fixture = Fixture::new();
    let (mut pa, mut pb) = start_same_session_pair(&fixture);
    let (fa, fb) = collect_race_results(&mut pa, &mut pb);
    let raw_a = oracle(&fixture.root, OP, &fixture.scratch);
    let raw_b = oracle(
        &fixture.root,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        &fixture.scratch,
    );
    assert_eq!(
        usize::from(raw_a != "null") + usize::from(raw_b != "null"),
        1,
        "same-session admission must have one durable winner: {raw_a} / {raw_b}"
    );
    assert_response_was_decisive(&fa);
    assert_response_was_decisive(&fb);
    assert_one_winner_effect(&fixture, raw_a, raw_b);
}

fn start_same_session_pair(fixture: &Fixture) -> (Owned, Owned) {
    let primer = fixture.plan_for(request_for(
        "dddddddddddddddddddddddddddddddd",
        "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        &fixed_generation(),
    ));
    let generation = fixture.prepare_only(primer).1;
    let (first_listener, first_socket) = fixture.bind(1);
    let (second_listener, second_socket) = fixture.bind(2);
    let session = "99999999999999999999999999999999";
    let mut a = fixture.plan_for(request_for(OP, session, &generation));
    a.initialize = false;
    a.gate = Some(Gate {
        stage: Stage::ParentBeforePrepare,
        path: first_socket.to_string_lossy().into_owned(),
    });
    let mut b = fixture.plan_for(request_for(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        session,
        &generation,
    ));
    b.initialize = false;
    b.gate = Some(Gate {
        stage: Stage::ParentBeforePrepare,
        path: second_socket.to_string_lossy().into_owned(),
    });
    let pa = fixture.parent(a);
    let pb = fixture.parent(b);
    let mut ga = gate_peer(&first_listener, Stage::ParentBeforePrepare, OP, NONCE);
    let mut gb = gate_peer(
        &second_listener,
        Stage::ParentBeforePrepare,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        NONCE,
    );
    echo_gate(&mut ga, Stage::ParentBeforePrepare, OP, NONCE);
    echo_gate(
        &mut gb,
        Stage::ParentBeforePrepare,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        NONCE,
    );
    drop((ga, gb));
    (pa, pb)
}

fn collect_race_results(pa: &mut Owned, pb: &mut Owned) -> (Frame, Frame) {
    let a = response(&pa.0);
    let b = response(&pb.0);
    finish_protocol(pa);
    finish_protocol(pb);
    (a, b)
}

fn assert_response_was_decisive(frame: &Frame) {
    assert!(matches!(
        frame,
        Frame::Accepted { .. }
            | Frame::Report {
                kind: ReportKind::Uncertain | ReportKind::Refused | ReportKind::Existing,
                ..
            }
    ));
}

fn assert_one_winner_effect(fixture: &Fixture, raw_a: String, raw_b: String) {
    let (winning_operation, winner) = if raw_a != "null" {
        (OP, raw_a)
    } else {
        ("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", raw_b)
    };
    assert_eq!(
        number(&winner, "phase"),
        Some(3),
        "winning operation must be accepted: {winner}"
    );
    let child = oracle_process(&winner, "child");
    let effects = fs::read_to_string(&fixture.effects).expect("effect ledger");
    assert_eq!(
        effects,
        format!(
            "{winning_operation}|{}|{}|{}|{}\n",
            child.pid(),
            child.start_seconds(),
            child.start_microseconds(),
            child.boot_uuid()
        ),
        "single effect must bind the accepted actual child identity"
    );
}

#[test]
fn observed_owner_fence_rejects_valid_replacement_generation_before_effect() {
    let fixture = Fixture::new();
    let effects = fixture.effects.clone();
    let (listener, socket) = fixture.bind(3);
    let mut plan = fixture.plan();
    plan.gate = Some(Gate {
        stage: Stage::OwnerAfterObserved,
        path: socket.to_string_lossy().into_owned(),
    });
    let mut parent = fixture.parent(plan);
    let mut peer = gate_peer(&listener, Stage::OwnerAfterObserved, OP, NONCE);
    let observed = oracle(&fixture.root, OP, &fixture.scratch);
    assert_eq!(number(&observed, "phase"), Some(2), "{observed}");
    let original_generation = oracle_generation(&observed);
    let aside = fixture.scratch.join("original-store");
    fs::rename(&fixture.root, &aside).expect("preserve original reserved store");
    let replacement = fixture.scratch.join("replacement");
    fs::create_dir(&replacement).expect("replacement parent");
    let replacement_root = replacement.join("store");
    let replacement_ledger_path = replacement.join("r03-b2.effects");
    let replacement_ledger = create_ledger(&replacement_ledger_path);
    let mut replacement_plan = fixture.plan_at(&replacement_root, replacement_ledger);
    replacement_plan.request = request_for(
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "cccccccccccccccccccccccccccccccc",
        &fixed_generation(),
    );
    let (_, replacement_generation) = fixture.prepare_only(replacement_plan);
    assert_ne!(
        original_generation, replacement_generation,
        "replacement store must have a different schema2 generation"
    );
    let replacement_before = oracle(
        &replacement_root,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        &replacement,
    );
    assert_eq!(
        number(&replacement_before, "phase"),
        Some(0),
        "replacement fixture starts independently prepared"
    );
    fs::rename(&replacement_root, &fixture.root).expect("install valid different-generation store");
    echo_gate(&mut peer, Stage::OwnerAfterObserved, OP, NONCE);
    let frame = response(&parent.0);
    assert!(
        matches!(
            frame,
            Frame::Report {
                kind: ReportKind::Uncertain,
                ..
            }
        ),
        "{frame:?}"
    );
    finish_protocol(&mut parent);
    let retained = oracle(&aside, OP, &fixture.scratch);
    assert_eq!(
        number(&retained, "phase"),
        Some(2),
        "original claim must remain reserved: {retained}"
    );
    assert_eq!(
        oracle(
            &fixture.root,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            &fixture.scratch
        ),
        replacement_before,
        "replacement ownership row must remain unchanged"
    );
    assert_empty(&effects);
    assert_empty(&replacement_ledger_path);
}

#[test]
fn direct_owner_loss_at_claim_child_spawn_and_observation_retains_exclusion() {
    for (index, stage, expected_phase, has_child) in [
        (4, Stage::OwnerAfterClaim, 1, false),
        (5, Stage::OwnerAfterChildSpawn, 1, true),
        (6, Stage::OwnerAfterObserved, 2, true),
    ] {
        let fixture = Fixture::new();
        let (_, generation) = fixture.prepare_only(fixture.plan());
        let (listener, socket) = fixture.bind(index);
        let mut plan = fixture.plan_for(request_for(OP, SESSION, &generation));
        plan.initialize = false;
        plan.gate = Some(Gate {
            stage,
            path: socket.to_string_lossy().into_owned(),
        });
        let mut owner = fixture.direct_owner(plan);
        let peer = gate_peer(&listener, stage, OP, NONCE);
        let owner_pid = owner.0.pid();
        let row = oracle(&fixture.root, OP, &fixture.scratch);
        assert_eq!(number(&row, "phase"), Some(expected_phase), "{row}");
        let child_pid = if has_child {
            Some(effect_child_pid(owner_pid, &fixture.scratch))
        } else {
            None
        };
        if stage == Stage::OwnerAfterClaim {
            assert!(
                row.contains("\"child\": null"),
                "claim precedes child spawn: {row}"
            );
        }
        if stage == Stage::OwnerAfterObserved {
            assert_eq!(
                oracle_process(&row, "child").pid(),
                child_pid.expect("actual effect child")
            );
        }
        assert_empty(&fixture.effects);
        kill_direct(&mut owner);
        drop(peer);
        if let Some(pid) = child_pid {
            wait_identity_gone(pid);
        }
        assert_empty(&fixture.effects);
        let after = oracle(&fixture.root, OP, &fixture.scratch);
        assert_eq!(
            number(&after, "phase"),
            Some(expected_phase),
            "owner loss never releases the claim: {after}"
        );
        let mut replay_plan = fixture.plan_for(request_for(OP, SESSION, &generation));
        replay_plan.initialize = false;
        let mut replay = fixture.parent(replay_plan);
        assert!(matches!(
            response(&replay.0),
            Frame::Report {
                kind: ReportKind::Existing,
                ..
            }
        ));
        finish_protocol(&mut replay);
        assert_empty(&fixture.effects);
    }
}

#[test]
fn cancel_intent_at_observed_fence_does_not_release_or_replay() {
    let fixture = Fixture::new();
    let (listener, socket) = fixture.bind(7);
    let mut plan = fixture.plan();
    plan.gate = Some(Gate {
        stage: Stage::OwnerAfterObserved,
        path: socket.to_string_lossy().into_owned(),
    });
    let mut parent = fixture.parent(plan);
    let mut peer = gate_peer(&listener, Stage::OwnerAfterObserved, OP, NONCE);
    let read = |name: &str| -> String {
        run_python_fixture(name, &fixture.root, OP, &fixture.scratch)
            .trim()
            .to_owned()
    };
    let before = read("read_pending.py");
    assert!(before.contains("\"phase\": 2"), "{before}");
    assert!(before.contains("\"revision\": 2"), "{before}");
    assert!(before.contains("\"cancelled\": 0"), "{before}");
    assert!(before.contains("\"release_proof\": null"), "{before}");
    assert_eq!(read("cancel_pending.py"), "1");
    let cancelled = read("read_pending.py");
    assert!(
        cancelled.contains("\"phase\": 2"),
        "cancellation preserves observed phase: {cancelled}"
    );
    assert!(
        cancelled.contains("\"revision\": 3"),
        "cancellation advances exactly one revision: {cancelled}"
    );
    assert!(cancelled.contains("\"cancelled\": 1"), "{cancelled}");
    assert!(
        cancelled.contains("\"release_proof\": null"),
        "cancel intent is not release evidence: {cancelled}"
    );
    echo_gate(&mut peer, Stage::OwnerAfterObserved, OP, NONCE);
    assert!(matches!(
        response(&parent.0),
        Frame::Report {
            kind: ReportKind::Uncertain,
            ..
        }
    ));
    finish_protocol(&mut parent);
    assert_eq!(
        read("read_pending.py"),
        cancelled,
        "stale start fence cannot mutate the cancellation row"
    );
    let observed = oracle(&fixture.root, OP, &fixture.scratch);
    assert!(
        observed.contains("\"probe_pid\": null"),
        "effect fence must refuse before probe/write: {observed}"
    );
    assert_empty(&fixture.effects);
    let generation = oracle_generation(&observed);
    let mut replay_plan = fixture.plan_for(request_for(OP, SESSION, &generation));
    replay_plan.initialize = false;
    let mut replay = fixture.parent(replay_plan);
    assert!(matches!(
        response(&replay.0),
        Frame::Report {
            kind: ReportKind::Existing,
            ..
        }
    ));
    finish_protocol(&mut replay);
    assert_eq!(read("read_pending.py"), cancelled);
    assert_empty(&fixture.effects);
}
