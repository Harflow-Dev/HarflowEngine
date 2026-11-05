#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[path = "isolated_owner/evidence.rs"]
mod evidence;
#[path = "isolated_owner/process.rs"]
mod process;
#[path = "isolated_owner/support.rs"]
mod support;

use evidence::{assert_root_busy, observe_agent, read_writer_identity, wait_arrived};
use hf_domain::fixture::{Gate, Stage};
use process::{CurrentDirectory, OwnerGuard};
use std::fs;
use support::{Scratch, deadline, self_identity};

/// The writer is an independent direct child of the owner. Once it reaches the
/// raw append gate, reaping the owner must not release the writer's root lease.
#[test]
fn owner_death_keeps_writer_lease_until_natural_writer_cleanup() {
    let scratch = Scratch::new(self_identity());
    let (listener, socket) = scratch.gate_listener();
    let stage = Stage::CaptureBeforeSync;
    let gate = Gate {
        stage,
        path: socket.to_string_lossy().into_owned(),
    };

    let original_cwd = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
    let cwd = CurrentDirectory::enter(&scratch.path);
    let mut owner = OwnerGuard::spawn(&scratch);
    let mut gate_peer = support::GatePeer::start(
        listener,
        stage,
        support::id(support::OP),
        support::id(support::NONCE),
    );
    owner.handshake(&scratch, gate);
    drop(cwd);
    assert_eq!(
        fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
        original_cwd
    );

    wait_arrived(&gate_peer, deadline(6));
    let writer = read_writer_identity(
        &scratch
            .root
            .join(format!("owner-participant-{}.v1", support::OP)),
    );
    owner.writer = Some(writer);
    let agent = observe_agent(&scratch);
    owner.agent = Some(agent);
    assert_root_busy(&scratch.root, deadline(2));

    owner.kill_and_reap();
    // The writer remains the lock holder after the original owner is reaped.
    assert_root_busy(&scratch.root, deadline(2));

    gate_peer.release();
    gate_peer
        .finish()
        .expect("writer gate completed and helper thread joined");
    owner.finish_writer(deadline(6));
    drop(owner);
    drop(scratch);
}
