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
