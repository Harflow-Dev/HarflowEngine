#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "../../tests/isolated_owner/support.rs"]
mod support;

use super::owner;
use hf_domain::fixture::{Prepared, Stage};
use hf_platform::{fixture::FIXED_STDERR, process::current_parent_identity};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

struct RestoreDirectory(PathBuf);
impl Drop for RestoreDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).expect("restore test working directory");
    }
}

#[test]
fn owner_keeps_draining_and_reaps_agent_while_writer_is_gated() {
    let _serial = crate::fixture::test_support::case_guard();
    let caller = current_parent_identity().expect("qualified direct caller identity");
    let scratch = support::Scratch::new(caller);
    let previous = std::env::current_dir().expect("test working directory");
    std::env::set_current_dir(&scratch.path).expect("set fixture physical working directory");
    let _restore = RestoreDirectory(previous);

    let (prepared, mut peer, mut owner) = start_gated_owner(&scratch);
    let mut gated_progress = None;
    let mut final_progress = None;
    let mut released = false;
    let drive = owner.drive(|progress| {
        final_progress = Some(*progress);
        if peer.arrived()
            && !released
            && progress.writer_pending
            && progress.agent_exit_observed
            && progress.reader_joined
        {
            gated_progress = Some(*progress);
            peer.release();
            released = true;
        }
    });
    peer.release();
    let gate_result = peer.finish();

    assert_raw_outputs(&scratch, &prepared);
    assert!(
        drive.is_ok(),
        "owner loop returned before settlement: {drive:?}"
    );
    assert!(
        gate_result.is_ok(),
        "finite storage gate failed: {gate_result:?}"
    );
    let gated =
        gated_progress.expect("writer remained pending after both pipes drained and child exited");
    assert!(gated.writer_pending);
    assert!(gated.agent_exit_observed);
    assert!(gated.reader_joined);
    assert!(
        !gated.reader_fifo_reconciled,
        "the pending nonempty append must remain outside the acknowledged FIFO prefix: {gated:?}"
    );
    assert!(final_progress.unwrap().reader_fifo_reconciled);

    assert_root_hold_lifetime(&scratch, owner);
}

fn start_gated_owner(
    scratch: &support::Scratch,
) -> (Prepared, support::GatePeer, owner::OwnerLoop) {
    let stage = Stage::CaptureBeforeSync;
    let (listener, socket) = scratch.gate_listener();
    let gate = hf_domain::fixture::Gate {
        stage,
        path: socket.to_string_lossy().into_owned(),
    };
    let prepared = scratch.prepared(support::self_identity(), gate);
    let peer = support::GatePeer::start(
        listener,
        stage,
        prepared.request.operation.clone(),
        prepared.invitation.nonce.clone(),
    );
    let executable = support::built_scaffold();
    assert!(executable.is_file(), "scaffold test helper must be built");
    let owner = owner::start_at(prepared.clone(), support::deadline(8), &executable)
        .expect("start actual isolated owner loop");
    (prepared, peer, owner)
}

fn assert_raw_outputs(scratch: &support::Scratch, prepared: &Prepared) {
    let raw_prefix = format!("r03-b2c.{}", prepared.request.operation.as_str());
    assert_eq!(
        fs::read(scratch.path.join(format!("{raw_prefix}.stdout"))).unwrap(),
        hf_platform::fixture::FIXED_STDOUT
    );
    assert_eq!(
        fs::read(scratch.path.join(format!("{raw_prefix}.stderr"))).unwrap(),
        FIXED_STDERR
    );
}

fn assert_root_hold_lifetime(scratch: &support::Scratch, owner: owner::OwnerLoop) {
    let lock = scratch.root.join("store.lock");
    let occupied = hf_platform::lease::shared().acquire(
        &lock,
        hf_application::lease::LeaseMode::Exclusive,
        false,
        support::deadline(1),
    );
    assert!(
        occupied.is_err(),
        "root hold must survive until owner drops"
    );
    drop(owner);
    let released = hf_platform::lease::shared().acquire(
        &lock,
        hf_application::lease::LeaseMode::Exclusive,
        false,
        Instant::now() + Duration::from_secs(1),
    );
    assert!(released.is_ok(), "owner drop must release its root hold");
}
