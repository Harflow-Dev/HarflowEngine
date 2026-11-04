use super::super::*;

fn rejected_proofs() -> [ReleaseProof; 5] {
    use hf_domain::ownership::CaptureAudit;
    [
        ReleaseProof::Terminal {
            evidence: id(30),
            owner_dead: false,
            child: process(202),
            exit: ProcessExit::Code(0),
            drain_complete: true,
            containment_complete: true,
            capture: CaptureAudit::Unknown,
        },
        ReleaseProof::Terminal {
            evidence: id(31),
            owner_dead: true,
            child: process(202),
            exit: ProcessExit::Code(0),
            drain_complete: false,
            containment_complete: true,
            capture: CaptureAudit::Unknown,
        },
        ReleaseProof::Terminal {
            evidence: id(32),
            owner_dead: true,
            child: process(202),
            exit: ProcessExit::Code(0),
            drain_complete: true,
            containment_complete: false,
            capture: CaptureAudit::Unknown,
        },
        ReleaseProof::Terminal {
            evidence: id(33),
            owner_dead: true,
            child: process(202),
            exit: ProcessExit::Unobserved,
            drain_complete: true,
            containment_complete: true,
            capture: CaptureAudit::Unknown,
        },
        ReleaseProof::Terminal {
            evidence: id(34),
            owner_dead: true,
            child: process(203),
            exit: ProcessExit::Code(0),
            drain_complete: true,
            containment_complete: true,
            capture: CaptureAudit::KnownLoss,
        },
    ]
}

fn reject_release(
    store: &Store,
    request: &PreparedRequest,
    fence: &hf_domain::ownership::Fence,
    proof: &ReleaseProof,
) {
    assert!(matches!(
        store.ownership_release(
            &request.operation,
            &request.generation,
            fence.revision,
            Some(fence),
            proof,
            deadline()
        ),
        Err(Error::InvalidOwnership)
    ));
}

fn complete_proof() -> ReleaseProof {
    ReleaseProof::Terminal {
        evidence: id(36),
        owner_dead: true,
        child: process(202),
        exit: ProcessExit::Signal(9),
        drain_complete: true,
        containment_complete: true,
        capture: hf_domain::ownership::CaptureAudit::KnownLoss,
    }
}

#[test]
fn terminal_release_requires_exact_child_exit_and_complete_drain_and_containment() {
    let fixture = Fixture::new("terminal-release");
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
    let spawned = store
        .ownership_record_spawn(&claimed.fence().unwrap(), &process(202), deadline())
        .unwrap();
    let fence = spawned.fence().unwrap();
    let signature = raw_record_signature(&store, &request.operation);
    let changes = store.connection.lock().unwrap().total_changes();
    for proof in rejected_proofs() {
        reject_release(&store, &request, &fence, &proof);
        assert_eq!(raw_record_signature(&store, &request.operation), signature);
    }
    let never_started = ReleaseProof::NeverStarted {
        evidence: id(35),
        owner_dead: true,
    };
    assert!(matches!(
        store.ownership_release(
            &request.operation,
            &request.generation,
            fence.revision,
            Some(&fence),
            &never_started,
            deadline()
        ),
        Err(Error::InvalidOwnership)
    ));
    assert_eq!(raw_record_signature(&store, &request.operation), signature);
    assert_eq!(store.connection.lock().unwrap().total_changes(), changes);
    let complete = complete_proof();
    let released = store
        .ownership_release(
            &request.operation,
            &request.generation,
            fence.revision,
            Some(&fence),
            &complete,
            deadline(),
        )
        .unwrap();
    assert_eq!(released.phase, Phase::Released);
    let observed = store
        .ownership_observe(&request.operation, deadline())
        .unwrap()
        .unwrap();
    assert_eq!(observed.phase, Phase::Released);
    let proof: Option<String> = store
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT release_proof FROM ownership_records WHERE operation=?1",
            [request.operation.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        proof
            .as_deref()
            .is_some_and(|value| value.contains("terminal:dead:signal:9:drained:contained:loss"))
    );
}
