use super::*;

fn stale_fences(fence: &hf_domain::ownership::Fence) -> Vec<hf_domain::ownership::Fence> {
    let mut values = Vec::new();
    let mut bad = fence.clone();
    bad.owner.owner = id(98);
    values.push(bad);
    let mut bad = fence.clone();
    bad.owner.nonce = id(97);
    values.push(bad);
    let mut bad = fence.clone();
    bad.owner.process = process(102);
    values.push(bad);
    let mut bad = fence.clone();
    bad.attempt = id(96);
    values.push(bad);
    let mut bad = fence.clone();
    bad.generation = id(99);
    values.push(bad);
    let mut bad = fence.clone();
    bad.revision += 1;
    values.push(bad);
    values
}

fn assert_stale_rejected(
    store: &Store,
    request: &PreparedRequest,
    fence: &hf_domain::ownership::Fence,
    stale: &hf_domain::ownership::Fence,
) {
    assert!(matches!(
        store.ownership_record_spawn(stale, &process(202), deadline()),
        Err(Error::StaleOwnership)
    ));
    assert!(matches!(
        store.ownership_record_acceptance(stale, deadline()),
        Err(Error::StaleOwnership)
    ));
    let proof = ReleaseProof::NeverStarted {
        evidence: id(95),
        owner_dead: true,
    };
    assert!(matches!(
        store.ownership_release(
            &request.operation,
            &request.generation,
            fence.revision,
            Some(stale),
            &proof,
            deadline()
        ),
        Err(Error::StaleOwnership)
    ));
}

#[test]
fn stale_fences_cannot_record_spawn_acceptance_or_release() {
    let fixture = Fixture::new("fence");
    let store = fixture.store();
    let request = request(&store, 2, 3, 4, 5, b"payload");
    store.ownership_prepare(&request, deadline()).unwrap();
    let ClaimResult::Granted(claimed) = store
        .ownership_claim(
            &request.operation,
            &request.generation,
            0,
            &owner(),
            deadline(),
        )
        .unwrap()
    else {
        panic!("grant")
    };
    let fence = claimed.fence().unwrap();
    let signature = raw_record_signature(&store, &request.operation);
    let before_changes = store.connection.lock().unwrap().total_changes();
    for stale in stale_fences(&fence) {
        assert_stale_rejected(&store, &request, &fence, &stale);
        assert_eq!(raw_record_signature(&store, &request.operation), signature);
    }
    assert_eq!(
        store.connection.lock().unwrap().total_changes(),
        before_changes
    );
    assert!(matches!(
        store.ownership_record_acceptance(&fence, deadline()),
        Err(Error::OwnershipConflict)
    ));
    let spawned = store
        .ownership_record_spawn(&fence, &process(202), deadline())
        .unwrap();
    assert_eq!(spawned.phase, Phase::SpawnObserved);
    assert!(matches!(
        store.ownership_record_acceptance(&fence, deadline()),
        Err(Error::StaleOwnership)
    ));
    let current = spawned.fence().unwrap();
    assert!(matches!(
        store.ownership_record_spawn(&current, &process(203), deadline()),
        Err(Error::OwnershipConflict)
    ));
    let proof = ReleaseProof::Terminal {
        evidence: id(20),
        owner_dead: true,
        child: process(202),
        exit: ProcessExit::Code(0),
        drain_complete: true,
        containment_complete: true,
        capture: hf_domain::ownership::CaptureAudit::KnownLoss,
    };
    assert!(matches!(
        store.ownership_release(
            &request.operation,
            &request.generation,
            fence.revision,
            Some(&fence),
            &proof,
            deadline()
        ),
        Err(Error::StaleOwnership)
    ));
    let accepted = store
        .ownership_record_acceptance(&current, deadline())
        .unwrap();
    assert_eq!(accepted.phase, Phase::AcceptanceRecorded);
}
