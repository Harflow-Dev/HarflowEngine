use super::support::*;
use hf_application::process::IdentityPort;
use hf_domain::{
    fixture::{FixtureFault, Frame, Gate, ParentPlan, ReportKind, Stage},
    ownership::PreparedRequest,
};
use hf_interfaces::fixture::encode;
use hf_platform::{fixture::gate_packet, process::MacIdentity};
use std::os::unix::net::UnixStream;
use std::{fs, os::unix::net::UnixListener};

#[test]
fn accepted_effect_records_actual_child_before_start_and_replay_adds_none() {
    let scratch = Scratch::new();
    let root = scratch.root();
    let effect_path = scratch.0.join("r03-b2.effects");
    let ledger = ledger(&effect_path);
    let (listener, socket) = bind_gate(&scratch, "observed");
    let gate = Gate {
        stage: Stage::OwnerAfterObserved,
        path: socket.to_string_lossy().into_owned(),
    };
    let mut parent = launch_parent(
        &scratch,
        &parent_plan(&root, ledger, Some(gate), FixtureFault::None),
    );
    let child_at_observation = verify_observed_gate(&listener, &root, &effect_path);
    let (invitation, child) = complete_parent(&mut parent);
    verify_acceptance(&root, &effect_path, &invitation, &child);
    replay_is_existing(
        &scratch,
        &root,
        ledger,
        invitation.generation,
        &effect_path,
        1,
    );
    assert_eq!(child, child_at_observation);
}

pub(super) fn bind_gate(scratch: &Scratch, label: &str) -> (UnixListener, std::path::PathBuf) {
    let path = scratch.0.join(format!("{label}.sock"));
    let listener = UnixListener::bind(&path).expect("bind isolated Unix gate");
    listener
        .set_nonblocking(true)
        .expect("nonblocking gate listener");
    (listener, path)
}

fn verify_observed_gate(
    listener: &UnixListener,
    root: &std::path::Path,
    effects: &std::path::Path,
) -> hf_domain::process::ProcessIdentity {
    let expected = gate_packet(Stage::OwnerAfterObserved, &id(OP), &id(NONCE));
    let mut peer = gate_peer(listener, &expected);
    let raw = run_oracle(root, OP, root.parent().expect("fixture parent"));
    assert!(
        raw.contains("\"phase\": 2"),
        "child observation was not persisted first: {raw}"
    );
    assert_eq!(
        oracle_probe_pid(&raw),
        None,
        "effect helper must stay inert until observed row persists"
    );
    assert_eq!(
        fs::read_to_string(effects).expect("read inert effect ledger"),
        ""
    );
    let child = oracle_process(&raw, "child");
    assert_eq!(
        MacIdentity.identity(child.pid()),
        Some(child.clone()),
        "row must bind the live inert OS child"
    );
    echo_gate(&mut peer, &expected);
    child
}

pub(super) fn gate_peer(listener: &UnixListener, packet: &[u8]) -> UnixStream {
    let mut peer = accept_until(listener, deadline(7)).expect("fixture reached selected gate");
    peer.set_nonblocking(true).expect("nonblocking gate peer");
    let mut received = vec![0; packet.len()];
    read_until(&mut peer, &mut received, deadline(1)).expect("read bounded gate packet");
    assert_eq!(received, packet);
    peer
}

pub(super) fn echo_gate(peer: &mut UnixStream, packet: &[u8]) {
    write_until(peer, packet, deadline(1)).expect("echo gate packet");
    peer.shutdown(std::net::Shutdown::Write)
        .expect("close gate response");
    expect_eof(peer, deadline(1)).expect("fixture gate closed after one exchange");
}

fn complete_parent(
    parent: &mut ChildGuard,
) -> (
    hf_domain::fixture::Invitation,
    hf_domain::process::ProcessIdentity,
) {
    let Frame::Accepted {
        invitation,
        fence,
        child,
    } = response(&parent.0)
    else {
        panic!("expected acceptance frame")
    };
    assert_eq!(invitation.process, fence.owner.process);
    assert_eq!(fence.owner.nonce, id(NONCE));
    assert!(child.pid() > 1 && child.pid() != invitation.process.pid());
    finish_protocol(parent);
    (invitation, child)
}

fn verify_acceptance(
    root: &std::path::Path,
    effects: &std::path::Path,
    invitation: &hf_domain::fixture::Invitation,
    child: &hf_domain::process::ProcessIdentity,
) {
    let raw = run_oracle(root, OP, root.parent().expect("fixture parent"));
    assert!(
        raw.contains("\"phase\": 3"),
        "acceptance missing from independent SQLite snapshot: {raw}"
    );
    assert_eq!(oracle_process(&raw, "owner_process"), invitation.process);
    assert_eq!(oracle_process(&raw, "child"), *child);
    assert_eq!(
        oracle_probe_pid(&raw),
        Some(child.pid()),
        "independent effect process must open, probe, and write after store handles close"
    );
    let ledger = fs::read_to_string(effects).expect("read fixed effect ledger");
    assert_eq!(ledger.lines().count(), 1);
    assert!(ledger.starts_with(&format!(
        "{OP}|{}|{}|{}|{}\n",
        child.pid(),
        child.start_seconds(),
        child.start_microseconds(),
        child.boot_uuid()
    )));
}

pub(super) fn replay_is_existing(
    scratch: &Scratch,
    root: &std::path::Path,
    ledger: hf_domain::fixture::QualifiedLedger,
    generation: hf_domain::ownership::Id,
    effects: &std::path::Path,
    expected_effects: usize,
) {
    let replay_request = PreparedRequest {
        generation,
        ..request()
    };
    let bytes = encode(&Frame::Parent(ParentPlan {
        root: root.to_string_lossy().into_owned(),
        request: replay_request,
        initialize: false,
        launch: true,
        owner: id(OWNER),
        nonce: id("88888888888888888888888888888888"),
        gate: None,
        ledger,
        fault: FixtureFault::None,
    }))
    .expect("encode replay plan");
    let mut replay = launch_parent(scratch, &bytes);
    assert!(matches!(
        response(&replay.0),
        Frame::Report {
            kind: ReportKind::Existing,
            ..
        }
    ));
    finish_protocol(&mut replay);
    assert_eq!(
        fs::read_to_string(effects)
            .expect("reread ledger")
            .lines()
            .count(),
        expected_effects
    );
}
