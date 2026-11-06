#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Each case starts from a ledger asserted valid, then breaks one rule.
mod conversation_support;
use conversation_support::*;
use hf_domain::conversation::{Error, Ledger, MessageState};

/// Consumed, claimed and queued messages; the live claim is the latest.
fn live() -> Ledger {
    let mut ledger = fresh();
    let first = issue(&mut ledger, 30, 31, 80);
    consume(&mut ledger, first, 40);
    ledger.send(send(&ledger, 32, "b")).unwrap();
    ledger.send(send(&ledger, 33, "c")).unwrap();
    ledger.record_boundary(boundary(&ledger, 12)).unwrap();
    ledger
        .claim(claim_intent(&ledger, 34, 81))
        .unwrap()
        .unwrap();
    ledger
}

/// Two consumed claims and a boundary; no live claim.
fn settled() -> Ledger {
    let mut ledger = live();
    let claim = ledger.messages[1].claim.clone().unwrap();
    consume(&mut ledger, claim, 41);
    ledger.record_boundary(boundary(&ledger, 13)).unwrap();
    ledger
}

/// Thirty-three send and cancel pairs: exactly one retirement.
fn compacted() -> Ledger {
    let mut ledger = fresh();
    churn(&mut ledger, 1000, 33);
    ledger
}

/// A claimed message stays retained at sequence 1 while later ones retire.
fn protected() -> Ledger {
    let mut ledger = fresh();
    ledger.send(send(&ledger, 30, "held")).unwrap();
    ledger.record_boundary(boundary(&ledger, 12)).unwrap();
    ledger
        .claim(claim_intent(&ledger, 31, 80))
        .unwrap()
        .unwrap();
    churn(&mut ledger, 1000, 40);
    ledger
}

fn assert_invalid(ledger: &Ledger) {
    assert_eq!(ledger.validate(), Err(Error::Invalid));
}

#[test]
fn baselines_are_valid() {
    for ledger in [live(), settled(), compacted(), protected()] {
        assert_eq!(ledger.validate(), Ok(()));
    }
    assert_eq!(live().messages[1].state, MessageState::Claimed);
    assert_eq!(live().messages[2].state, MessageState::Queued);
    let ledger = compacted();
    assert_eq!(ledger.retired_messages, 1);
    assert_eq!(ledger.ledger_epoch, 1);
    assert_eq!(ledger.history_floor, 2);
}

#[test]
fn live_claim_must_match_the_ledger_turn() {
    let mut ledger = live();
    assert_eq!(ledger.validate(), Ok(()));
    ledger.turn = id(999);
    assert_invalid(&ledger);
}

#[test]
fn retained_claim_at_the_current_generation_must_match_the_ledger_turn() {
    let mut ledger = settled();
    assert_eq!(ledger.validate(), Ok(()));
    ledger.turn = id(999);
    ledger.boundary.as_mut().unwrap().expected.turn = id(999);
    assert_invalid(&ledger);
}

#[test]
fn retained_claim_generations_must_strictly_increase() {
    let mut ledger = live();
    assert_eq!(ledger.validate(), Ok(()));
    let current = ledger.messages[1].claim.clone().unwrap();
    // Give the earlier claim the same generation and turn as the later one.
    let first = &mut ledger.messages[0];
    let claim = first.claim.as_mut().unwrap();
    claim.generation = current.generation;
    claim.turn = current.turn.clone();
    claim.intent.destination = current.turn.clone();
    claim.intent.expected.generation = current.generation - 1;
    first.receipt.as_mut().unwrap().claim = claim.clone();
    assert_invalid(&ledger);
}

#[test]
fn a_message_with_a_claim_may_not_follow_a_queued_message() {
    let mut ledger = live();
    assert_eq!(ledger.validate(), Ok(()));
    ledger.messages.swap(1, 2);
    ledger.messages[1].sequence = 2;
    ledger.messages[2].sequence = 3;
    assert_eq!(ledger.messages[1].state, MessageState::Queued);
    assert_eq!(ledger.messages[2].state, MessageState::Claimed);
    assert_invalid(&ledger);
}

#[test]
fn cumulative_issuances_cannot_exceed_admitted_messages() {
    let mut ledger = settled();
    assert_eq!(ledger.validate(), Ok(()));
    let extra = 3;
    ledger.generation += extra;
    ledger.cumulative_issuances += extra;
    ledger.unavailable_usage += extra;
    ledger.boundary.as_mut().unwrap().expected.generation = ledger.generation;
    assert!(ledger.cumulative_issuances > ledger.next_sequence - 1);
    assert_invalid(&ledger);
}

#[test]
fn history_floor_must_follow_the_highest_absent_sequence() {
    for floor in [1, 3] {
        let mut ledger = compacted();
        assert_eq!(ledger.validate(), Ok(()));
        ledger.history_floor = floor;
        assert_invalid(&ledger);
    }
}

#[test]
fn protected_entry_may_sit_below_the_history_floor() {
    let ledger = protected();
    assert_eq!(ledger.validate(), Ok(()));
    assert_eq!(ledger.messages[0].sequence, 1);
    assert_eq!(ledger.messages[0].state, MessageState::Claimed);
    assert!(ledger.history_floor > ledger.messages[0].sequence);
    // Sequence 1 is retained, so the retired sequences are 2..=retired + 1.
    assert_eq!(ledger.history_floor, ledger.retired_messages + 2);
    let mut oldest_first = ledger.clone();
    oldest_first.history_floor = ledger.retired_messages + 1;
    assert_invalid(&oldest_first);
}

#[test]
fn epoch_cannot_exceed_retired_messages() {
    let mut ledger = compacted();
    assert_eq!(ledger.validate(), Ok(()));
    ledger.ledger_epoch = ledger.retired_messages + 1;
    assert_invalid(&ledger);
}

#[test]
fn retirement_requires_a_nonzero_epoch() {
    let mut ledger = compacted();
    assert_eq!(ledger.validate(), Ok(()));
    ledger.ledger_epoch = 0;
    // Witnesses recorded after the retirement also named epoch 1.
    for message in &mut ledger.messages {
        if let Some(cancel) = message.cancellation.as_mut() {
            cancel.expected.ledger_epoch = 0;
        }
    }
    assert_invalid(&ledger);
}

#[test]
fn counters_near_u64_max_are_rejected_without_overflow() {
    let mut ledger = compacted();
    ledger.next_sequence = u64::MAX;
    assert_invalid(&ledger);
    let mut ledger = compacted();
    ledger.retired_messages = u64::MAX;
    assert_invalid(&ledger);
    let mut ledger = settled();
    ledger.cumulative_issuances = u64::MAX;
    assert_invalid(&ledger);
    let mut ledger = settled();
    ledger.next_sequence = 0;
    assert_invalid(&ledger);
}
