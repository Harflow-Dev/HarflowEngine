#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

// Real private fixture children and disposable roots only. Typed coordinator and
// command-history assertions belong to the sibling application/owner unit tests.
#[path = "handoff_losses/support.rs"]
#[allow(dead_code, unused_imports)]
mod base;
#[path = "isolated_owner/support.rs"]
#[allow(dead_code)]
mod preparation;
#[path = "b8_storage_refusal/support.rs"]
mod support;

use hf_domain::fixture::{Gate, Stage};
use support::*;

#[test]
fn fence_between_appends_preserves_the_last_committed_capture() {
    exercise(Fault::Fence, Stage::CaptureAfterPublish);
}

#[test]
fn commit_lost_between_appends_preserves_the_last_committed_capture() {
    exercise(Fault::CommitLost, Stage::CaptureAfterPublish);
}

fn exercise(fault: Fault, stage: Stage) {
    let _serial = support::case_guard();
    // Preserve the root on assertion failure: a failed settlement must never
    // trigger fixture teardown while an original descendant may remain alive.
    let scratch =
        std::mem::ManuallyDrop::new(preparation::Scratch::new(preparation::self_identity()));
    let _cwd = Directory::enter(&scratch.path);
    let (listener, socket) = scratch.gate_listener();
    let gate = Gate {
        stage,
        path: socket.to_string_lossy().into_owned(),
    };
    let mut owner = start_owner(&scratch, gate);
    let mut peer = base::gate_peer(&listener, stage, preparation::OP, preparation::NONCE);
    let owner_pid = owner.0.registered_identity().unwrap().pid();
    let writer_pid = base::process_child_pid(
        owner_pid,
        "--internal-storage-writer-fixture-v1",
        &scratch.path,
    );
    let before = snapshot(&scratch);
    let child_pid = before.lines().nth(1).unwrap().parse::<u32>().unwrap();
    assert_lease_busy(&scratch.root);
    fault.inject(&scratch);
    base::echo_gate(&mut peer, stage, preparation::OP, preparation::NONCE);
    settle_refused_owner(&mut owner);
    base::wait_identity_gone(writer_pid);
    base::wait_identity_gone(child_pid);
    assert_eq!(
        snapshot(&scratch),
        before,
        "refusal must not advance commit sequence, capture revision/events or ownership"
    );
    assert_lease_available(&scratch.root);
    assert_eq!(
        std::fs::read_to_string(scratch.path.join("r03-b2.effects"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    drop(_cwd);
    drop(std::mem::ManuallyDrop::into_inner(scratch));
}
