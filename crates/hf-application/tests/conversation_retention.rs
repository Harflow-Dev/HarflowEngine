#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod conversation_support;
use conversation_support::*;
use hf_domain::conversation::{
    Error, Ledger, MAX_OUTSTANDING, MAX_PROSE, MAX_RECEIPTS, MAX_TEXT, MessageState,
};

fn has(ledger: &Ledger, request: u32) -> bool {
    ledger
        .messages
        .iter()
        .any(|m| m.intent.request == id(request))
}

#[test]
fn consumed_latest_claim_is_protected_until_a_boundary_exists() {
    let mut ledger = fresh();
    let claim = issue(&mut ledger, 30, 31, 80);
    consume(&mut ledger, claim, 40);
    churn(&mut ledger, 1000, 40);
    assert!(has(&ledger, 30), "latest consumed message is retained");
    assert!(ledger.retired_messages > 0);
    ledger.record_boundary(boundary(&ledger, 12)).unwrap();
    churn(&mut ledger, 2000, 2);
    assert!(
        !has(&ledger, 30),
        "eligible after the boundary, oldest first"
    );
    assert_eq!(ledger.validate(), Ok(()));
}

#[test]
fn claimed_and_unknown_messages_are_never_retired() {
    for mark_unknown in [false, true] {
        let mut ledger = fresh();
        let claim = issue(&mut ledger, 30, 31, 80);
        if mark_unknown {
            ledger.unknown(&claim).unwrap();
        }
        churn(&mut ledger, 1000, 60);
        let expected = if mark_unknown {
            MessageState::Unknown
        } else {
            MessageState::Claimed
        };
        assert_eq!(ledger.messages[0].intent.request, id(30));
        assert_eq!(ledger.messages[0].state, expected);
        assert_eq!(ledger.validate(), Ok(()));
    }
}

#[test]
fn epoch_advance_and_floor_track_retirement() {
    let mut ledger = fresh();
    churn(&mut ledger, 1000, 32);
    assert_eq!((ledger.ledger_epoch, ledger.retired_messages), (0, 0));
    assert_eq!(ledger.history_floor, 1);
    churn(&mut ledger, 2000, 1);
    assert_eq!((ledger.ledger_epoch, ledger.retired_messages), (1, 1));
    assert_eq!(ledger.history_floor, 2);
    assert!(ledger.history_gaps);
    assert!(ledger.settled_receipts() <= MAX_RECEIPTS);
}

#[test]
fn receipt_pressure_drains_thirty_two_queued_without_new_sends() {
    let mut ledger = fresh();
    churn(&mut ledger, 1000, 31);
    for n in 0..32 {
        ledger.send(send(&ledger, 100 + n, "q")).unwrap();
    }
    assert_eq!(ledger.outstanding(), MAX_OUTSTANDING);
    ledger.record_boundary(boundary(&ledger, 12)).unwrap();
    for n in 0..32 {
        let claim = ledger
            .claim(claim_intent(&ledger, 200 + n, 300 + n))
            .unwrap()
            .unwrap();
        assert_eq!(claim.message, id(100 + n), "FIFO order");
        consume(&mut ledger, claim, 400 + n);
        ledger.record_boundary(boundary(&ledger, 500 + n)).unwrap();
        assert_eq!(ledger.validate(), Ok(()));
        assert!(ledger.settled_receipts() <= MAX_RECEIPTS);
    }
    assert_eq!(ledger.outstanding(), 0);
    assert_eq!(ledger.cumulative_issuances, 32);
    assert!(ledger.retired_messages > 0);
    assert!(ledger.ledger_epoch > 0);
}

#[test]
fn retained_uuid_replays_across_epochs_and_retired_uuid_is_expired() {
    let mut ledger = fresh();
    let original = send(&ledger, 30, "first");
    ledger.send(original.clone()).unwrap();
    ledger.cancel(cancel_intent(&ledger, 31, 30)).unwrap();
    let old_cancel = cancel_intent(&fresh(), 31, 30);
    churn(&mut ledger, 1000, 40);
    assert!(ledger.ledger_epoch > 0);
    // Message 30 is the oldest cancelled entry, so it has been retired.
    assert!(!has(&ledger, 30));
    assert_eq!(ledger.send(original.clone()), Err(Error::Expired));
    assert_eq!(ledger.cancel(old_cancel), Err(Error::Expired));
    // A retained entry from epoch 0 still replays exactly.
    let retained = ledger
        .messages
        .iter()
        .find(|m| m.intent.expected.ledger_epoch == 0)
        .unwrap()
        .intent
        .clone();
    let before = ledger.clone();
    assert_eq!(ledger.send(retained.clone()), Ok(()));
    assert_eq!(ledger, before);
    // Changed intent on a retained UUID conflicts.
    let mut changed = retained;
    changed.text.push('!');
    assert_eq!(ledger.send(changed), Err(Error::Conflict));
}

#[test]
fn retired_uuid_is_reusable_at_the_current_epoch_but_retained_uuid_is_not() {
    let mut ledger = fresh();
    churn(&mut ledger, 1000, 40);
    assert!(!has(&ledger, 1000));
    assert!(has(&ledger, 1000 + 2 * 39));
    ledger.send(send(&ledger, 1000, "reused")).unwrap();
    assert!(has(&ledger, 1000));
    assert_eq!(
        ledger.send(send(&ledger, 1000 + 2 * 39, "reused")),
        Err(Error::Conflict)
    );
    assert_eq!(ledger.validate(), Ok(()));
}

#[test]
fn text_length_bounds() {
    let mut ledger = fresh();
    ledger
        .send(send(&ledger, 30, &"x".repeat(MAX_TEXT)))
        .unwrap();
    assert_eq!(
        ledger.send(send(&ledger, 31, &"x".repeat(MAX_TEXT + 1))),
        Err(Error::Invalid)
    );
    assert_eq!(ledger.send(send(&ledger, 32, "")), Err(Error::Invalid));
    assert_eq!(ledger.messages.len(), 1);
}

#[test]
fn thirty_two_outstanding_fill_the_prose_budget_and_the_next_is_refused() {
    let mut ledger = fresh();
    for n in 0..32 {
        ledger
            .send(send(&ledger, 100 + n, &"x".repeat(MAX_TEXT)))
            .unwrap();
    }
    assert_eq!(ledger.retained_prose(), MAX_PROSE);
    assert_eq!(ledger.validate(), Ok(()));
    let before = ledger.clone();
    assert_eq!(ledger.send(send(&ledger, 200, "x")), Err(Error::Capacity));
    assert_eq!(ledger, before);
}

#[test]
fn escape_heavy_text_is_stored_verbatim() {
    let text = "\u{0}\u{1}\n\r\t\"\\\u{7f}\u{1b}[0m \u{2028}\u{fffd}\"\\\\n";
    let mut ledger = fresh();
    ledger.send(send(&ledger, 30, text)).unwrap();
    assert_eq!(ledger.messages[0].intent.text, text);
}

#[test]
fn protected_consumed_message_blocks_prose_retirement_until_a_boundary_exists() {
    let mut ledger = fresh();
    let text = "x".repeat(MAX_TEXT);
    for n in 0..32 {
        ledger.send(send(&ledger, 100 + n, &text)).unwrap();
    }
    assert_eq!(ledger.retained_prose(), MAX_PROSE);
    ledger.record_boundary(boundary(&ledger, 12)).unwrap();
    let claim = ledger
        .claim(claim_intent(&ledger, 200, 300))
        .unwrap()
        .unwrap();
    assert_eq!(claim.message, id(100));
    consume(&mut ledger, claim, 400);
    assert_eq!(ledger.outstanding(), MAX_OUTSTANDING - 1);
    assert!(ledger.boundary.is_none());

    // The latest consumed message is protected, so nothing can be retired.
    let before = ledger.clone();
    assert_eq!(ledger.send(send(&ledger, 500, &text)), Err(Error::Capacity));
    assert_eq!(ledger, before);
    assert_eq!(ledger.ledger_epoch, 0);

    // Once a boundary is observed the consumed message becomes eligible.
    ledger.record_boundary(boundary(&ledger, 13)).unwrap();
    ledger.send(send(&ledger, 500, &text)).unwrap();
    assert!(!has(&ledger, 100));
    assert_eq!(ledger.ledger_epoch, 1);
    assert_eq!(ledger.retired_messages, 1);
    assert_eq!(ledger.validate(), Ok(()));
}
