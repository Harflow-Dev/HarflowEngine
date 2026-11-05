#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[path = "tests/support.rs"]
mod support;
use super::*;
use hf_application::owner::OwnerSession;
use hf_domain::ownership::OwnerClaim;
use hf_platform::process::{CaptureOrigin, MacIdentity};
use hf_storage::{Limits, Store};
use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use support::*;

mod poison;

#[test]
fn fixed_owner_drains_actual_pipes_while_capture_begin_is_gated_and_persists_known_loss_prefix() {
    let _serial = case_guard();
    let scratch = Scratch::new();
    let until = Instant::now() + Duration::from_secs(10);
    let store = Store::initialize_fixture(
        scratch.root(),
        Limits::default(),
        until,
        hf_platform::lease::shared(),
    )
    .unwrap();
    let request = request(&store);
    store.ownership_prepare(&request, until).unwrap();
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let gate = Arc::new(CaptureGate::new(held_tx, release_rx));
    let (gated_store, calls) = GatedStore::new(store, gate.clone(), false);
    let owner = OwnerClaim {
        owner: id(0x55),
        nonce: id(0x66),
        process: MacIdentity.identity(std::process::id()).unwrap(),
    };
    let mut session = OwnerSession::new(gated_store, request.clone(), owner, 0);
    session.claim(until).unwrap();
    let witness = scratch.0.join("producer.finished");
    let child = child(&witness);
    let mut owned = OwnerChild::new(child, until);
    let identity = owned.child.registered_identity().unwrap().clone();
    session.record_child(&identity, until).unwrap();
    let raw = RawCapture::create(&scratch.root(), &request.operation, until).unwrap();
    let origin = CaptureOrigin::from_spawn_observed(session.snapshot().unwrap()).unwrap();
    owned.reader = Some(
        owned
            .child
            .start_private_fixture_capture_reader(origin.clone(), until)
            .unwrap(),
    );
    gate.set_view(owned.reader.as_ref().unwrap().view());
    session.accept(until).unwrap();
    let binding = accepted_binding(&session, &origin, &raw).unwrap();
    let mut worker = spawn_owner(owned, raw, session, binding, release_tx, until);

    let view = held_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    wait_progress(&view, &witness);
    worker.release.send(()).unwrap();
    assert_known_loss(worker.join().unwrap(), &calls, &scratch, &request);
}
