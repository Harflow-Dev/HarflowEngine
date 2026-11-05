use super::fixture::{Refusal, validate_accepted, validate_prepared};
use hf_domain::{
    fixture::{FixtureFault, Gate, Invitation, Prepared, QualifiedLedger, Stage},
    ownership::{Digest, Id, OwnerClaim, Phase, PreparedRequest, Snapshot},
    process::ProcessIdentity,
};

fn id(byte: char) -> Id {
    Id::new(byte.to_string().repeat(32)).unwrap()
}
fn digest(byte: char) -> Digest {
    Digest::new(byte.to_string().repeat(64)).unwrap()
}
fn process(pid: u32, start: u64) -> ProcessIdentity {
    ProcessIdentity::new(
        pid,
        start,
        23,
        "abcdef01-2345-6789-abcd-ef0123456789".into(),
    )
    .unwrap()
}
fn request() -> PreparedRequest {
    PreparedRequest {
        operation: id('1'),
        session: id('2'),
        reservation: id('3'),
        attempt: id('4'),
        generation: id('5'),
        digest: digest('a'),
        payload: b"fixed-operation".to_vec(),
    }
}
fn invitation(owner_process: ProcessIdentity) -> Invitation {
    Invitation {
        operation: id('1'),
        generation: id('5'),
        digest: digest('a'),
        owner: id('6'),
        nonce: id('7'),
        process: owner_process,
        revision: 0,
    }
}
fn prepared(invite: Invitation) -> Prepared {
    Prepared {
        invitation: invite,
        request: request(),
        root: "/private/tmp/root".into(),
        gate: Some(Gate {
            stage: Stage::OwnerAfterClaim,
            path: "/private/tmp/gate.sock".into(),
        }),
        ledger: QualifiedLedger {
            device: 11,
            inode: 12,
        },
        fault: FixtureFault::None,
    }
}
fn snapshot(
    request: PreparedRequest,
    phase: Phase,
    revision: u64,
    owner: Option<OwnerClaim>,
    child: Option<ProcessIdentity>,
) -> Snapshot {
    Snapshot {
        request,
        phase,
        revision,
        cancelled: false,
        owner,
        child,
    }
}
fn owner(invite: &Invitation) -> OwnerClaim {
    OwnerClaim {
        owner: invite.owner.clone(),
        nonce: invite.nonce.clone(),
        process: invite.process.clone(),
    }
}

#[test]
fn prepared_handoff_requires_full_invitation_actual_owner_and_stored_request() {
    let actual = process(41, 90);
    let invite = invitation(actual.clone());
    let prepared = prepared(invite.clone());
    let stored = snapshot(request(), Phase::Prepared, 0, None, None);
    assert_eq!(
        validate_prepared(&invite, &prepared, &actual, &stored),
        Ok(())
    );

    let mut altered = prepared.clone();
    altered.invitation.nonce = id('8');
    assert_eq!(
        validate_prepared(&invite, &altered, &actual, &stored),
        Err(Refusal::Binding)
    );
    for change in 0..3 {
        let mut altered = prepared.clone();
        match change {
            0 => altered.request.operation = id('9'),
            1 => altered.request.generation = id('9'),
            _ => altered.request.digest = digest('b'),
        }
        assert_eq!(
            validate_prepared(&invite, &altered, &actual, &stored),
            Err(Refusal::Binding)
        );
    }
    assert_eq!(
        validate_prepared(&invite, &prepared, &process(41, 91), &stored),
        Err(Refusal::Binding)
    );

    let mut other_payload = request();
    other_payload.payload.push(0);
    assert_eq!(
        validate_prepared(
            &invite,
            &prepared,
            &actual,
            &snapshot(other_payload, Phase::Prepared, 0, None, None)
        ),
        Err(Refusal::StoredRequest)
    );
    assert_eq!(
        validate_prepared(
            &invite,
            &prepared,
            &actual,
            &snapshot(request(), Phase::Prepared, 1, None, None)
        ),
        Err(Refusal::StoredRequest)
    );
    assert_eq!(
        validate_prepared(
            &invite,
            &prepared,
            &actual,
            &snapshot(request(), Phase::Claimed, 0, Some(owner(&invite)), None)
        ),
        Err(Refusal::State)
    );
    let mut cancelled = stored.clone();
    cancelled.cancelled = true;
    assert_eq!(
        validate_prepared(&invite, &prepared, &actual, &cancelled),
        Err(Refusal::State)
    );
}

#[test]
fn acceptance_requires_exact_owner_nonce_generation_revision_and_child_fence() {
    let actual = process(41, 90);
    let invite = invitation(actual.clone());
    let child = process(42, 91);
    let claim = owner(&invite);
    let accepted = snapshot(
        request(),
        Phase::AcceptanceRecorded,
        3,
        Some(claim),
        Some(child.clone()),
    );
    let fence = accepted.fence().unwrap();
    assert_eq!(
        validate_accepted(&invite, &fence, &child, &actual, &accepted),
        Ok(())
    );

    let wrong_child = process(42, 92);
    assert_eq!(
        validate_accepted(&invite, &fence, &wrong_child, &actual, &accepted),
        Err(Refusal::Binding)
    );
    assert_eq!(
        validate_accepted(&invite, &fence, &child, &process(41, 91), &accepted),
        Err(Refusal::Binding)
    );
    let mut wrong_fence = fence.clone();
    wrong_fence.owner.nonce = id('9');
    assert_eq!(
        validate_accepted(&invite, &wrong_fence, &child, &actual, &accepted),
        Err(Refusal::Binding)
    );
    wrong_fence = fence.clone();
    wrong_fence.revision += 1;
    assert_eq!(
        validate_accepted(&invite, &wrong_fence, &child, &actual, &accepted),
        Err(Refusal::Binding)
    );
    let mut not_accepted = accepted.clone();
    not_accepted.phase = Phase::SpawnObserved;
    assert_eq!(
        validate_accepted(&invite, &fence, &child, &actual, &not_accepted),
        Err(Refusal::State)
    );
}
