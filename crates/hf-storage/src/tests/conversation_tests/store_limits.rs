//! Retention and numeric bounds through the real SQLite store.
use super::fixture::{
    Fixture, apply, cancel_intent, churn, claim_intent, deadline, evidence, id, observe, receipt,
    seed, send_intent,
};
use crate::{Error, Limits, tests::fixture_guard};
use hf_application::conversation::Command;
use hf_domain::conversation::{Error as DomainError, MAX_PROSE, MAX_TEXT};

#[test]
fn more_than_sixty_four_sequential_messages_survive_real_commits_and_reopen() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("rolling");
    let store = fixture.store();
    let enrollment = seed(&store);
    store.conversation_enroll(&enrollment, deadline()).unwrap();
    let mut ledger = observe(&store, &enrollment);
    ledger = apply(
        &store,
        &enrollment,
        Command::Boundary(evidence(&ledger, 12)),
    );
    for n in 0..70u32 {
        let base = 1000 + n * 10;
        ledger = apply(
            &store,
            &enrollment,
            Command::Send(send_intent(&ledger, base, "m")),
        );
        let outcome = store
            .conversation_apply(
                &enrollment.conversation,
                Command::Claim(claim_intent(&ledger, base + 1, base + 2)),
                deadline(),
            )
            .unwrap();
        let claim = outcome.grant.expect("one grant per cycle").into_claim();
        assert_eq!(claim.message, send_intent(&ledger, base, "m").request);
        ledger = apply(&store, &enrollment, receipt(claim, base + 3));
        ledger = apply(
            &store,
            &enrollment,
            Command::Boundary(evidence(&ledger, base + 4)),
        );
    }
    assert_eq!(ledger.cumulative_issuances, 70);
    assert!(ledger.retired_messages > 0 && ledger.ledger_epoch > 0);
    drop(store);
    let reopened = observe(&fixture.reopen(), &enrollment);
    assert_eq!(reopened, ledger);
    assert_eq!(reopened.enrollment.ordinary_turns, 14);
    assert_eq!(reopened.enrollment.node_attempts, 6);
}

#[test]
fn completion_reserve_refuses_admission_but_not_cancellation() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("reserve");
    let tight = Limits {
        max_rows: 128,
        max_bytes: hf_domain::conversation::COMPLETION_RESERVE + 2048,
    };
    let store = fixture.store_with(tight);
    let enrollment = seed(&store);
    store.conversation_enroll(&enrollment, deadline()).unwrap();
    let text = "x".repeat(100);
    let mut accepted = 0u32;
    let refusal = loop {
        assert!(accepted < 31, "reserve never refused an admission");
        let ledger = observe(&store, &enrollment);
        let intent = send_intent(&ledger, 30 + accepted, &text);
        match store.conversation_apply(&enrollment.conversation, Command::Send(intent), deadline())
        {
            Ok(_) => accepted += 1,
            Err(error) => break error,
        }
    };
    assert!(accepted > 0);
    assert!(matches!(refusal, Error::LimitExceeded), "{refusal:?}");
    let bytes = fixture.ledger_bytes(&enrollment);
    assert_eq!(observe(&store, &enrollment).messages.len() as u32, accepted);
    // Commands that never admit prose are not charged the reserve.
    let ledger = observe(&store, &enrollment);
    let cancelled = apply(
        &store,
        &enrollment,
        Command::Cancel(cancel_intent(&ledger, 900, 30)),
    );
    assert_eq!(cancelled.messages.len() as u32, accepted);
    assert_ne!(fixture.ledger_bytes(&enrollment), bytes);
}

#[test]
fn full_prose_budget_round_trips_and_the_next_admission_is_capacity() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("prose");
    let store = fixture.store();
    let enrollment = seed(&store);
    store.conversation_enroll(&enrollment, deadline()).unwrap();
    let text = "y".repeat(MAX_TEXT);
    for n in 0..32 {
        let ledger = observe(&store, &enrollment);
        apply(
            &store,
            &enrollment,
            Command::Send(send_intent(&ledger, 30 + n, &text)),
        );
    }
    let full = observe(&store, &enrollment);
    assert_eq!(full.retained_prose(), MAX_PROSE);
    let refused = store.conversation_apply(
        &enrollment.conversation,
        Command::Send(send_intent(&full, 99, "z")),
        deadline(),
    );
    assert!(
        matches!(refused, Err(Error::Conversation(DomainError::Capacity))),
        "{refused:?}"
    );
    drop(store);
    assert_eq!(observe(&fixture.reopen(), &enrollment), full);
}

#[test]
fn escape_heavy_text_is_stored_byte_for_byte() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("escape");
    let store = fixture.store();
    let enrollment = seed(&store);
    store.conversation_enroll(&enrollment, deadline()).unwrap();
    let text = "\u{0}\u{1}\n\r\t\"\\\u{7f}\u{1b}[0m \u{2028}\u{fffd}\"\\\\n {\"a\":1}";
    let ledger = observe(&store, &enrollment);
    apply(
        &store,
        &enrollment,
        Command::Send(send_intent(&ledger, 30, text)),
    );
    drop(store);
    let reopened = observe(&fixture.reopen(), &enrollment);
    assert_eq!(reopened.messages[0].intent.text.as_bytes(), text.as_bytes());
}

#[test]
fn claim_replay_survives_an_epoch_bump_while_retained_and_expires_once_retired() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("claim-replay");
    let store = fixture.store();
    let enrollment = seed(&store);
    store.conversation_enroll(&enrollment, deadline()).unwrap();
    let mut ledger = observe(&store, &enrollment);
    ledger = apply(
        &store,
        &enrollment,
        Command::Send(send_intent(&ledger, 30, "m")),
    );
    ledger = apply(
        &store,
        &enrollment,
        Command::Boundary(evidence(&ledger, 12)),
    );
    let intent = claim_intent(&ledger, 31, 80);
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
    apply(&store, &enrollment, receipt(claim, 40));

    // No boundary yet, so the latest consumed message stays retained.
    let bumped = churn(&store, &enrollment, 1000, 40);
    assert!(bumped.ledger_epoch > 0);
    assert!(bumped.messages.iter().any(|m| m.intent.request == id(30)));
    let bytes = fixture.ledger_bytes(&enrollment);
    let replay = store
        .conversation_apply(
            &enrollment.conversation,
            Command::Claim(intent.clone()),
            deadline(),
        )
        .unwrap();
    assert!(replay.grant.is_none());
    assert_eq!(replay.ledger.cumulative_issuances, 1);
    assert_eq!(fixture.ledger_bytes(&enrollment), bytes);

    // After a boundary the consumed message is the oldest eligible entry.
    apply(
        &store,
        &enrollment,
        Command::Boundary(evidence(&bumped, 13)),
    );
    let retired = churn(&store, &enrollment, 2000, 2);
    assert!(!retired.messages.iter().any(|m| m.intent.request == id(30)));
    let bytes = fixture.ledger_bytes(&enrollment);
    let result =
        store.conversation_apply(&enrollment.conversation, Command::Claim(intent), deadline());
    assert!(
        matches!(result, Err(Error::Conversation(DomainError::Expired))),
        "{result:?}"
    );
    assert_eq!(fixture.ledger_bytes(&enrollment), bytes);
    assert_eq!(observe(&store, &enrollment).cumulative_issuances, 1);
}
