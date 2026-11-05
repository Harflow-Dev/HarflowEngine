use super::*;

#[test]
fn lost_publish_reply_poisons_writer_and_never_mutates_after_ambiguous_commit() {
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
    let capture_gate = Arc::new(CaptureGate::new(held_tx, release_rx));
    let (gated_store, calls) = GatedStore::new(store, capture_gate.clone(), true);
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
    capture_gate.set_view(owned.reader.as_ref().unwrap().view());
    session.accept(until).unwrap();
    let binding = accepted_binding(&session, &origin, &raw).unwrap();
    let mut worker = spawn_owner(owned, raw, session, binding, release_tx, until);

    let view = held_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    wait_progress(&view, &witness);
    let observed = view.snapshot();
    worker.release.send(()).unwrap();
    assert_poisoned_publish(
        worker.join().unwrap(),
        &observed,
        &calls,
        &scratch,
        &request,
    );
}
