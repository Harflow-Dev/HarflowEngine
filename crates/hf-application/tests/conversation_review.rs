#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Claim replay across retirement and receipt correlation.
mod conversation_support;
use conversation_support::*;
use hf_domain::conversation::{Claim, ClaimIntent, Error, Ledger, MessageState, Receipt};

type Edit = fn(&mut Claim);

fn has(ledger: &Ledger, request: u32) -> bool {
    ledger
        .messages
        .iter()
        .any(|m| m.intent.request == id(request))
}

/// Message 30 claimed (intent returned) and consumed; no boundary yet.
fn consumed() -> (Ledger, ClaimIntent) {
    let mut ledger = fresh();
    ledger.send(send(&ledger, 30, "one")).unwrap();
    ledger.record_boundary(boundary(&ledger, 12)).unwrap();
    let intent = claim_intent(&ledger, 31, 80);
    let claim = ledger.claim(intent.clone()).unwrap().unwrap();
    consume(&mut ledger, claim, 40);
    (ledger, intent)
}

#[test]
fn retained_claim_replays_without_a_grant_across_an_epoch_bump() {
    let (mut ledger, intent) = consumed();
    churn(&mut ledger, 1000, 40);
    assert!(ledger.ledger_epoch > 0);
    assert!(has(&ledger, 30), "latest consumed message stays retained");
    let before = ledger.clone();
    assert_eq!(ledger.claim(intent), Ok(None));
    assert_eq!(ledger, before);
    assert_eq!(ledger.cumulative_issuances, 1);
}

#[test]
fn retired_claim_replay_is_expired_without_a_grant() {
    let (mut ledger, intent) = consumed();
    ledger.record_boundary(boundary(&ledger, 13)).unwrap();
    churn(&mut ledger, 1000, 40);
    assert!(!has(&ledger, 30), "consumed message retired once eligible");
    assert!(ledger.ledger_epoch > 0);
    let before = ledger.clone();
    assert_eq!(ledger.claim(intent), Err(Error::Expired));
    assert_eq!(ledger, before);
    assert_eq!(ledger.cumulative_issuances, 1);
    assert_eq!(ledger.unavailable_usage, 1);
}

#[test]
fn receipt_must_match_every_claim_field() {
    let mut ledger = fresh();
    let claim = issue(&mut ledger, 30, 31, 80);
    let mut cases = Vec::new();
    let edits: [(&str, Edit); 9] = [
        ("context", |c| c.context = id(999)),
        ("generation", |c| c.generation += 1),
        ("turn", |c| c.turn = id(999)),
        ("message", |c| c.message = id(999)),
        ("source", |c| c.source = id(999)),
        ("intent.request", |c| c.intent.request = id(999)),
        ("intent.expected", |c| c.intent.expected.generation += 1),
        ("intent.destination", |c| c.intent.destination = id(999)),
        ("intent.expected.epoch", |c| {
            c.intent.expected.ledger_epoch += 1;
        }),
    ];
    for (field, edit) in edits {
        let mut bad = claim.clone();
        edit(&mut bad);
        assert_ne!(bad, claim, "{field}");
        cases.push((field, bad));
    }
    for (field, bad) in cases {
        let before = ledger.clone();
        let receipt = Receipt {
            claim: bad,
            evidence: id(40),
        };
        assert_eq!(ledger.receipt(receipt), Err(Error::Conflict), "{field}");
        assert_eq!(ledger, before, "{field}");
        assert_eq!(ledger.messages[0].state, MessageState::Claimed);
    }
    consume(&mut ledger, claim, 40);
    assert_eq!(ledger.messages[0].state, MessageState::Consumed);
}
