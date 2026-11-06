#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! Low-level owner/writer gates only. These cases do not model workflow,
//! session/message/worktree obligations or dispatcher callback closure.
#[allow(dead_code)]
#[path = "isolated_owner/evidence.rs"]
mod evidence;
#[allow(dead_code)]
#[path = "isolated_owner/process.rs"]
mod process;
#[allow(dead_code)]
#[path = "isolated_owner/support.rs"]
mod support;

use evidence::{assert_root_busy, read_writer_identity, wait_arrived, wait_root_available};
use hf_domain::fixture::{Gate, Stage};
use hf_storage::{Limits, Store};
use process::{CurrentDirectory, OwnerGuard};
use std::{fs, mem::ManuallyDrop};
use support::{Scratch, deadline};

fn capture_gate_is_finite_and_root_stays_owned(stage: Stage) {
    let scratch = ManuallyDrop::new(Scratch::new(support::self_identity()));
    let (listener, socket) = scratch.gate_listener();
    let gate = Gate {
        stage,
        path: socket.to_string_lossy().into_owned(),
    };
    let cwd = CurrentDirectory::enter(&scratch.path);
    let mut owner = OwnerGuard::spawn(&scratch);
    let mut peer = support::GatePeer::start(
        listener,
        stage,
        support::id(support::OP),
        support::id(support::NONCE),
    );
    owner.handshake(&scratch, gate);
    drop(cwd);

    wait_arrived(&peer, deadline(6));
    owner.writer = Some(read_writer_identity(
        &scratch
            .root
            .join(format!("owner-participant-{}.v1", support::OP)),
    ));
    owner.agent = Some(observe_original_agent(&scratch));
    assert_root_busy(&scratch.root, deadline(2));
    peer.release();
    peer.finish().expect("writer crossed selected finite gate");

    // OwnerGuard waits for and reaps the original owner; its Drop leaves the
    // fixture intact if any owned actor or lease remains uncertain.
    drop(owner);
    wait_root_available(&scratch.root, deadline(8));
    assert_raw_and_final_capture(&scratch);
    drop(ManuallyDrop::into_inner(scratch));
}

fn observe_original_agent(scratch: &Scratch) -> hf_domain::process::ProcessIdentity {
    let store = Store::open(
        &scratch.root,
        Limits::default(),
        deadline(2),
        hf_platform::lease::shared(),
    )
    .expect("open bounded ownership observation");
    let snapshot = store
        .ownership_observe(&support::id(support::OP), deadline(2))
        .expect("observe durable owner record")
        .expect("owner record present");
    let agent = snapshot.child.expect("original agent identity retained");
    drop(store);
    agent
}

fn assert_raw_and_final_capture(scratch: &Scratch) {
    let prefix = format!("r03-b2c.{}", support::OP);
    assert_eq!(
        fs::read(scratch.path.join(format!("{prefix}.stdout"))).expect("read committed stdout"),
        hf_platform::fixture::FIXED_STDOUT
    );
    assert_eq!(
        fs::read(scratch.path.join(format!("{prefix}.stderr"))).expect("read committed stderr"),
        hf_platform::fixture::FIXED_STDERR
    );
    let store = Store::open(
        &scratch.root,
        Limits::default(),
        deadline(2),
        hf_platform::lease::shared(),
    )
    .expect("open final capture observation");
    let capture = store
        .capture_observe(&support::id(support::OP), deadline(2))
        .expect("observe final capture")
        .expect("capture record present");
    assert!(capture.progress.stdout_eof && capture.progress.stderr_eof);
    assert!(!capture.progress.loss, "fixed owner capture is lossless");
    assert_eq!(
        capture.progress.stdout,
        hf_platform::fixture::FIXED_STDOUT.len() as u64
    );
    assert_eq!(
        capture.progress.stderr,
        hf_platform::fixture::FIXED_STDERR.len() as u64
    );
    assert_eq!(capture.exit, hf_domain::process::ProcessExit::Code(0));
}

#[test]
#[ignore = "requires Q0 review and explicit low-level owner/writer qualification run"]
fn capture_before_sync_gate_holds_root_until_writer_closes() {
    capture_gate_is_finite_and_root_stays_owned(Stage::CaptureBeforeSync);
}

#[test]
#[ignore = "requires Q0 review and explicit low-level owner/writer qualification run"]
fn capture_after_sync_gate_holds_root_until_writer_closes() {
    capture_gate_is_finite_and_root_stays_owned(Stage::CaptureAfterSync);
}

#[test]
#[ignore = "requires Q0 review and explicit low-level owner/writer qualification run"]
fn capture_after_publish_gate_holds_root_until_writer_closes() {
    capture_gate_is_finite_and_root_stays_owned(Stage::CaptureAfterPublish);
}

#[test]
#[ignore = "requires Q0 review and explicit low-level owner/writer qualification run"]
fn capture_before_terminal_gate_holds_root_until_writer_closes() {
    capture_gate_is_finite_and_root_stays_owned(Stage::CaptureBeforeTerminal);
}

#[test]
#[ignore = "requires Q0 review and explicit low-level owner/writer qualification run"]
fn capture_after_terminal_gate_holds_root_until_writer_closes() {
    capture_gate_is_finite_and_root_stays_owned(Stage::CaptureAfterTerminal);
}
