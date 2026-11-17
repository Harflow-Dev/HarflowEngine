use super::support::*;

#[test]
fn coordinator_admits_one_exact_pending_command_and_advances_sequence_on_full_ack() {
    let mut owner = coordinator();
    let pending = owner
        .begin(bootstrap(), digest('c'), deadline(), Instant::now())
        .unwrap();
    let first = pending.identity.clone();
    assert_eq!(first.sequence, 1);
    assert_eq!(
        owner
            .begin(bootstrap(), digest('d'), deadline(), Instant::now())
            .unwrap_err(),
        CoordinatorError::Pending
    );
    owner.send_complete(&first, Instant::now()).unwrap();
    owner
        .acknowledge(
            ack(first, 0, prepared_snapshot(), None, None),
            Instant::now(),
        )
        .unwrap();
    assert!(owner.pending().is_none());
    let close = owner
        .begin(
            StorageCommand::Close,
            digest('e'),
            deadline(),
            Instant::now(),
        )
        .unwrap();
    assert_eq!(close.identity.sequence, 2);
}
#[test]
fn mismatched_ack_poison_is_permanent_and_retains_pending_identity() {
    let mut owner = coordinator();
    let identity = owner
        .begin(bootstrap(), digest('c'), deadline(), Instant::now())
        .unwrap()
        .identity
        .clone();
    owner.send_complete(&identity, Instant::now()).unwrap();
    let mut wrong = identity.clone();
    wrong.sequence += 1;
    assert_eq!(
        owner
            .acknowledge(
                ack(wrong, 0, prepared_snapshot(), None, None),
                Instant::now(),
            )
            .unwrap_err(),
        CoordinatorError::InvalidAck
    );
    assert!(owner.is_poisoned());
    assert_eq!(owner.pending().unwrap().identity, identity);
    assert_eq!(
        owner
            .acknowledge(
                ack(identity, 0, prepared_snapshot(), None, None),
                Instant::now(),
            )
            .unwrap_err(),
        CoordinatorError::Poisoned
    );
}

#[test]
fn definite_storage_refusal_preserves_evidence_sequence_and_blocks_commands() {
    for cause in [StorageRefusal::Fenced, StorageRefusal::CommitLost] {
        let (mut owner, _, _) = established_coordinator();
        let before_snapshot = owner.snapshot().cloned();
        let before_capture = owner.capture().cloned();
        let before_raw = owner.raw_files().cloned();
        let before_sequence = owner.next_sequence();
        let pending = owner
            .begin(
                StorageCommand::Append {
                    stream: Stream::Stdout,
                    bytes: vec![1],
                    drain: None,
                },
                digest('c'),
                deadline(),
                Instant::now(),
            )
            .unwrap();
        let identity = pending.identity.clone();
        owner.send_complete(&identity, Instant::now()).unwrap();
        owner
            .acknowledge(
                refused_ack(
                    identity,
                    cause,
                    before_snapshot.clone(),
                    before_capture.clone(),
                    before_raw.clone(),
                ),
                Instant::now(),
            )
            .unwrap();

        assert_eq!(owner.storage_refusal(), Some(cause));
        assert_eq!(owner.snapshot().cloned(), before_snapshot);
        assert_eq!(owner.capture().cloned(), before_capture);
        assert_eq!(owner.raw_files().cloned(), before_raw);
        assert_eq!(owner.next_sequence(), before_sequence);
        assert!(owner.pending().is_none());
        assert!(matches!(
            owner.begin(
                StorageCommand::Close,
                digest('d'),
                deadline(),
                Instant::now()
            ),
            Err(CoordinatorError::StorageRefused)
        ));
    }
}

#[test]
fn contradictory_refusal_payload_cannot_classify_storage_as_definitely_refused() {
    let mut owner = coordinator();
    let identity = owner
        .begin(bootstrap(), digest('c'), deadline(), Instant::now())
        .unwrap()
        .identity
        .clone();
    owner.send_complete(&identity, Instant::now()).unwrap();
    let mut ack = refused_ack(identity, StorageRefusal::Fenced, None, None, None);
    ack.revision = Some(0);
    assert_eq!(
        owner.acknowledge(ack, Instant::now()),
        Err(CoordinatorError::InvalidAck)
    );
    assert_eq!(owner.storage_refusal(), None);
}

#[test]
fn ambiguous_unknown_ack_poisoning_does_not_record_a_refusal_cause() {
    let mut owner = coordinator();
    let identity = owner
        .begin(bootstrap(), digest('c'), deadline(), Instant::now())
        .unwrap()
        .identity
        .clone();
    owner.send_complete(&identity, Instant::now()).unwrap();
    let mut ack = ack(identity, 0, prepared_snapshot(), None, None);
    ack.disposition = AckDisposition::Unknown;
    assert_eq!(
        owner.acknowledge(ack, Instant::now()),
        Err(CoordinatorError::InvalidAck)
    );
    assert!(owner.is_poisoned());
    assert_eq!(owner.storage_refusal(), None);
}

#[test]
fn stale_or_expired_refusal_ack_cannot_classify_storage_as_definitely_refused() {
    let mut stale = coordinator();
    let identity = stale
        .begin(bootstrap(), digest('c'), deadline(), Instant::now())
        .unwrap()
        .identity
        .clone();
    stale.send_complete(&identity, Instant::now()).unwrap();
    let mut wrong = identity;
    wrong.sequence += 1;
    assert_eq!(
        stale.acknowledge(
            refused_ack(wrong, StorageRefusal::Fenced, None, None, None),
            Instant::now()
        ),
        Err(CoordinatorError::InvalidAck)
    );
    assert_eq!(stale.storage_refusal(), None);

    let mut expired = coordinator();
    let until = Instant::now() + Duration::from_millis(10);
    let identity = expired
        .begin(bootstrap(), digest('c'), until, Instant::now())
        .unwrap()
        .identity
        .clone();
    expired.send_complete(&identity, Instant::now()).unwrap();
    let ack = refused_ack(identity, StorageRefusal::CommitLost, None, None, None);
    assert_eq!(
        expired.acknowledge(ack, until),
        Err(CoordinatorError::Expired)
    );
    assert_eq!(expired.storage_refusal(), None);
}
