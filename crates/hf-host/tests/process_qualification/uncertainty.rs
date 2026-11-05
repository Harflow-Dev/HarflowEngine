use super::success::{bind_gate, echo_gate, gate_peer, replay_is_existing};
use super::support::*;
use hf_domain::fixture::{FixtureFault, Frame, Gate, ReportKind, Stage};
use hf_interfaces::fixture::decode;
use hf_platform::{fixture::gate_packet, process::MacIdentity};
use std::fs;

#[test]
fn uncertain_child_initialization_fault_retains_claim_and_prevents_replay() {
    assert_uncertain_claim(FixtureFault::UnknownChildIdentity);
}

#[test]
fn unusable_child_pipe_fault_retains_claim_and_prevents_replay() {
    assert_uncertain_claim(FixtureFault::UnusableChildPipe);
}

fn assert_uncertain_claim(fault: FixtureFault) {
    let scratch = Scratch::new();
    let root = scratch.root();
    let effects = scratch.0.join("r03-b2.effects");
    let ledger = ledger(&effects);
    let mut parent = launch_parent(&scratch, &parent_plan(&root, ledger, None, fault));
    let Frame::Report {
        generation,
        kind: ReportKind::Uncertain,
        ..
    } = response(&parent.0)
    else {
        panic!("fault must remain uncertain")
    };
    finish_protocol(&mut parent);
    let raw = run_oracle(&root, OP, &scratch.0);
    assert!(
        raw.contains("\"phase\": 1"),
        "claim must remain reserved: {raw}"
    );
    assert!(
        raw.contains("\"child\": null"),
        "unknown/unusable child is not persisted as observed"
    );
    assert_eq!(
        fs::read_to_string(&effects)
            .expect("empty ledger")
            .lines()
            .count(),
        0
    );
    replay_is_existing(&scratch, &root, ledger, generation, &effects, 0);
}

#[test]
fn accepted_but_unacknowledged_owner_keeps_reservation_and_replay_effect_count() {
    let scratch = Scratch::new();
    let root = scratch.root();
    let effects = scratch.0.join("r03-b2.effects");
    let ledger = ledger(&effects);
    let (listener, socket) = bind_gate(&scratch, "before-ack");
    let gate = Gate {
        stage: Stage::OwnerBeforeAck,
        path: socket.to_string_lossy().into_owned(),
    };
    let mut parent = launch_parent(
        &scratch,
        &parent_plan(&root, ledger, Some(gate), FixtureFault::None),
    );
    let expected = gate_packet(Stage::OwnerBeforeAck, &id(OP), &id(NONCE));
    let mut peer = gate_peer(&listener, &expected);
    prove_accepted_before_ack(&root, &effects);
    kill_owned_parent(&mut parent);
    echo_gate(&mut peer, &expected);
    let raw = run_oracle(&root, OP, &scratch.0);
    let generation = oracle_generation(&raw);
    assert!(
        raw.contains("\"phase\": 3"),
        "ack loss must not undo durable acceptance: {raw}"
    );
    assert_eq!(
        fs::read_to_string(&effects)
            .expect("read ledger")
            .lines()
            .count(),
        1
    );
    replay_is_existing(&scratch, &root, ledger, generation, &effects, 1);
}

#[test]
fn late_ack_deadline_retains_acceptance_and_replay_does_not_repeat_effect() {
    let scratch = Scratch::new();
    let root = scratch.root();
    let effects = scratch.0.join("r03-b2.effects");
    let ledger = ledger(&effects);
    let (listener, socket) = bind_gate(&scratch, "late-ack");
    let gate = Gate {
        stage: Stage::OwnerBeforeAck,
        path: socket.to_string_lossy().into_owned(),
    };
    let mut parent = launch_parent(
        &scratch,
        &parent_plan(&root, ledger, Some(gate), FixtureFault::None),
    );
    let expected = gate_packet(Stage::OwnerBeforeAck, &id(OP), &id(NONCE));
    let mut peer = gate_peer(&listener, &expected);
    prove_accepted_before_ack(&root, &effects);
    expect_eof(&mut peer, deadline(9)).expect("owner closes gate at its absolute local deadline");
    let frame = decode(
        &parent
            .0
            .read_fixture_frame(deadline(2))
            .expect("bounded uncertainty response after late ack"),
    )
    .expect("valid parent response");
    let Frame::Report {
        generation,
        kind: ReportKind::Uncertain,
        ..
    } = frame
    else {
        panic!("late or missing acknowledgment must remain uncertain")
    };
    parent
        .0
        .fixture_eof(deadline(2))
        .expect("parent response EOF");
    parent.finish();
    let raw = run_oracle(&root, OP, &scratch.0);
    assert!(
        raw.contains("\"phase\": 3"),
        "late ack must preserve durable acceptance: {raw}"
    );
    assert_eq!(
        fs::read_to_string(&effects)
            .expect("read fixed ledger")
            .lines()
            .count(),
        1
    );
    replay_is_existing(&scratch, &root, ledger, generation, &effects, 1);
}

fn prove_accepted_before_ack(root: &std::path::Path, effects: &std::path::Path) {
    let raw = run_oracle(root, OP, root.parent().expect("fixture parent"));
    assert!(
        raw.contains("\"phase\": 3"),
        "acceptance must persist before ack gate: {raw}"
    );
    assert_eq!(
        fs::read_to_string(effects)
            .expect("ledger before ack")
            .lines()
            .count(),
        1
    );
}

fn kill_owned_parent(parent: &mut ChildGuard) {
    let identity = parent
        .0
        .registered_identity()
        .expect("qualified direct parent")
        .clone();
    assert!(matches!(
        parent.0.request_signal(
            &identity,
            &MacIdentity,
            hf_platform::process::OwnedSignal::Kill,
            deadline(1)
        ),
        Ok(hf_platform::process::SignalOutcome::Sent)
    ));
    assert!(matches!(
        parent.0.wait_until(deadline(2), || false),
        Ok(hf_platform::process::WaitOutcome::Exited(
            hf_domain::process::ProcessExit::Signal(9)
        ))
    ));
}
