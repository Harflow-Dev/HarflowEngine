use super::support::*;

#[test]
fn expired_pending_command_poisons_without_clearing_or_advancing_it() {
    let mut owner = coordinator();
    let at = Instant::now();
    let identity = owner
        .begin(bootstrap(), digest('c'), at + Duration::from_millis(1), at)
        .unwrap()
        .identity
        .clone();
    assert_eq!(
        owner
            .check_deadline(at + Duration::from_secs(1))
            .unwrap_err(),
        CoordinatorError::Expired
    );
    assert!(owner.is_poisoned());
    assert_eq!(owner.pending().unwrap().identity, identity);
}
#[test]
fn expired_before_send_poisons_and_retains_unsent_command_identity() {
    let mut owner = coordinator();
    let now = Instant::now();
    let expiry = now + Duration::from_millis(10);
    let identity = owner
        .begin(bootstrap(), digest('c'), expiry, now)
        .unwrap()
        .identity
        .clone();
    assert_eq!(
        owner.send_complete(&identity, expiry + Duration::from_nanos(1)),
        Err(CoordinatorError::InvalidState)
    );
    assert!(owner.is_poisoned());
    assert!(!owner.pending().unwrap().sent);
    assert_eq!(owner.pending().unwrap().identity, identity);
    assert_eq!(owner.next_sequence(), 1);
}
#[test]
fn begin_at_or_before_deadline_refuses_and_poisons_before_admission() {
    let mut owner = coordinator();
    let now = Instant::now();
    assert!(matches!(
        owner.begin(bootstrap(), digest('c'), now, now),
        Err(CoordinatorError::Expired)
    ));
    assert!(owner.is_poisoned());
    assert!(owner.pending().is_none());
    assert_eq!(owner.next_sequence(), 1);
}
