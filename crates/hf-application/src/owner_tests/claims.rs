use super::support::*;
use crate::owner::OwnerError;

#[test]
fn claim_binds_full_prepared_request_revision_and_grant_before_enabling_mutations() {
    let (mut session, calls, _) = harness(Fault::None);
    assert!(session.snapshot().is_none());
    session.claim(deadline()).unwrap();
    assert_eq!(&*calls.borrow(), &[Call::Observe, Call::Claim]);
    let claimed = session.snapshot().unwrap();
    assert_eq!(claimed.request, request());
    assert_eq!(claimed.phase, Phase::Claimed);
    assert_eq!(claimed.revision, 1);
    assert_eq!(claimed.owner, Some(owner()));
    assert!(matches!(
        session.accept(deadline()),
        Err(OwnerError::InvalidState)
    ));
    assert!(session.is_poisoned());
}

#[test]
fn no_grant_and_ambiguous_or_malformed_writes_poison_without_dropping_port() {
    for fault in [
        Fault::NoGrant,
        Fault::Error(Call::Claim),
        Fault::Malformed(Call::Claim),
    ] {
        let (mut session, calls, dropped) = harness(fault);
        let error = session.claim(deadline()).unwrap_err();
        assert!(matches!(
            error,
            OwnerError::NoGrant | OwnerError::Port(_) | OwnerError::InvalidResponse
        ));
        assert!(session.is_poisoned());
        assert_eq!(
            session.record_child(&process(42), deadline()),
            Err(OwnerError::Poisoned)
        );
        assert_eq!(&*calls.borrow(), &[Call::Observe, Call::Claim]);
        assert!(!*dropped.borrow(), "poisoned session retains its port");
        drop(session);
        assert!(*dropped.borrow());
    }
}

#[test]
fn mismatched_prepared_snapshot_refuses_before_claim() {
    // Every request field, revision, phase, and cancellation bit is exact-bound.
    for field in 0..10 {
        let (mut session, calls, dropped) = harness(Fault::PreparedMismatch(field));
        assert_eq!(
            session.claim(deadline()),
            Err(OwnerError::InvalidResponse),
            "changed field {field}"
        );
        assert_eq!(&*calls.borrow(), &[Call::Observe]);
        assert!(session.is_poisoned());
        assert!(!*dropped.borrow());
    }
}
