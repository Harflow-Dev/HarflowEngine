//! Conversation history after the source operation is released.
use super::fixture::{
    Fixture, apply, claim_intent, deadline, id, observe, prime, receipt, release_source,
    send_intent,
};
use crate::{Error, tests::fixture_guard};
use hf_application::conversation::Command;
use hf_domain::{conversation::MessageState, ownership::Phase};

#[test]
fn released_source_keeps_history_replay_and_receipt_but_refuses_new_work() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("release");
    let enrollment = prime(&fixture, 1, true);
    let store = fixture.reopen();
    let before = observe(&store, &enrollment);
    let outcome = store
        .conversation_apply(
            &enrollment.conversation,
            Command::Claim(claim_intent(&before, 150, 170)),
            deadline(),
        )
        .unwrap();
    let claim = outcome.grant.unwrap().into_claim();
    apply(&store, &enrollment, Command::Unknown(claim.clone()));
    release_source(&store, &enrollment);

    let snapshot = store
        .ownership_observe(&enrollment.source.operation, deadline())
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.phase, Phase::Released);
    assert!(snapshot.revision >= enrollment.fence.revision);

    // History and exact replays still work.
    let history = observe(&store, &enrollment);
    assert_eq!(history.messages[0].state, MessageState::Unknown);
    let replay = apply(
        &store,
        &enrollment,
        Command::Claim(claim_intent(&before, 150, 170)),
    );
    assert_eq!(replay, history);
    let resend = send_intent(&before, 30, "m");
    assert_eq!(apply(&store, &enrollment, Command::Send(resend)), history);

    // New work is refused without changing the ledger.
    let bytes = fixture_bytes(&fixture, &enrollment);
    let current = observe(&store, &enrollment);
    for command in [
        Command::Send(send_intent(&current, 31, "new")),
        Command::Claim(claim_intent(&current, 151, 171)),
    ] {
        let result = store.conversation_apply(&enrollment.conversation, command, deadline());
        assert!(matches!(result, Err(Error::StaleOwnership)), "{result:?}");
    }
    assert_eq!(fixture_bytes(&fixture, &enrollment), bytes);

    // A matching receipt and a late unknown report both settle cleanly.
    let consumed = apply(&store, &enrollment, receipt(claim.clone(), 40));
    assert_eq!(consumed.messages[0].state, MessageState::Consumed);
    let late = apply(&store, &enrollment, Command::Unknown(claim));
    assert_eq!(late, consumed);
}

fn fixture_bytes(fixture: &Fixture, e: &hf_domain::conversation::Enrollment) -> Vec<u8> {
    fixture.ledger_bytes(e)
}

#[test]
fn released_source_refuses_a_new_claim_after_the_receipt() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("release-claim");
    let enrollment = prime(&fixture, 2, true);
    let store = fixture.reopen();
    let before = observe(&store, &enrollment);
    let claim = store
        .conversation_apply(
            &enrollment.conversation,
            Command::Claim(claim_intent(&before, 150, 170)),
            deadline(),
        )
        .unwrap()
        .grant
        .unwrap()
        .into_claim();
    release_source(&store, &enrollment);
    let consumed = apply(&store, &enrollment, receipt(claim, 40));
    assert_eq!(consumed.messages[0].state, MessageState::Consumed);
    let result = store.conversation_apply(
        &enrollment.conversation,
        Command::Claim(claim_intent(&consumed, 151, 171)),
        deadline(),
    );
    assert!(matches!(result, Err(Error::StaleOwnership)), "{result:?}");
    assert_eq!(id(30), consumed.messages[0].intent.request);
}

#[test]
fn a_source_row_below_the_enrolled_revision_refuses_observation() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("below-floor");
    let enrollment = prime(&fixture, 0, false);
    {
        let raw = fixture.raw_write();
        let changed = raw
            .execute(
                "UPDATE ownership_records SET revision=0 WHERE operation=?1",
                [enrollment.source.operation.as_str()],
            )
            .unwrap();
        assert_eq!(changed, 1);
    }
    let store = fixture.reopen();
    // Other source checks may fire first; only the refusal is asserted.
    assert!(
        store
            .conversation_observe(&enrollment.conversation, deadline())
            .is_err()
    );
}

/// Cancelling the source changes its revision and sets `cancelled`. For an
/// accepted row, `revision_valid` makes the fence comparison redundant with the
/// cancelled flag, because cancelled is true exactly when the fence mismatches.
/// This test proves the combined refusal, not each clause separately.
#[test]
fn cancelled_source_keeps_history_and_replay_but_refuses_new_work() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("cancelled");
    let enrollment = prime(&fixture, 2, true);
    let store = fixture.reopen();
    let before = observe(&store, &enrollment);
    let intent = claim_intent(&before, 150, 170);
    let claim = store
        .conversation_apply(
            &enrollment.conversation,
            Command::Claim(intent.clone()),
            deadline(),
        )
        .unwrap()
        .grant
        .unwrap()
        .into_claim();
    let snapshot = store
        .ownership_observe(&enrollment.source.operation, deadline())
        .unwrap()
        .unwrap();
    let cancelled = store
        .ownership_cancel(
            &enrollment.source.operation,
            &enrollment.source.generation,
            snapshot.revision,
            deadline(),
        )
        .unwrap();
    assert!(cancelled.cancelled);
    assert_ne!(cancelled.fence(), Some(enrollment.fence.clone()));

    let history = observe(&store, &enrollment);
    assert_eq!(history.messages[0].state, MessageState::Claimed);
    assert_eq!(apply(&store, &enrollment, Command::Claim(intent)), history);
    let resend = send_intent(&before, 30, "m");
    assert_eq!(apply(&store, &enrollment, Command::Send(resend)), history);

    let bytes = fixture.ledger_bytes(&enrollment);
    for command in [
        Command::Send(send_intent(&history, 41, "new")),
        Command::Claim(claim_intent(&history, 151, 171)),
    ] {
        let result = store.conversation_apply(&enrollment.conversation, command, deadline());
        assert!(matches!(result, Err(Error::StaleOwnership)), "{result:?}");
    }
    assert_eq!(fixture.ledger_bytes(&enrollment), bytes);
    let consumed = apply(&store, &enrollment, receipt(claim, 40));
    assert_eq!(consumed.messages[0].state, MessageState::Consumed);
}
