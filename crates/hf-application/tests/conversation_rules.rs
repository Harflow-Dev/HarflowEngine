#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod conversation_support;
use conversation_support::*;
use hf_domain::conversation::{Error, Ledger, MessageState, Receipt, Witness};

fn with_epoch(w: &Witness, epoch: u64) -> Witness {
    Witness {
        ledger_epoch: epoch,
        ..w.clone()
    }
}

/// Epoch 0 ledger with a boundary and one queued message.
fn ready() -> Ledger {
    let mut ledger = fresh();
    ledger.send(send(&ledger, 30, "one")).unwrap();
    ledger.record_boundary(boundary(&ledger, 12)).unwrap();
    ledger
}

#[test]
fn unknown_on_consumed_is_an_unchanged_ok() {
    let mut ledger = fresh();
    let claim = issue(&mut ledger, 30, 31, 80);
    ledger.unknown(&claim).unwrap();
    assert_eq!(ledger.messages[0].state, MessageState::Unknown);
    consume(&mut ledger, claim.clone(), 40);
    let before = ledger.clone();
    assert_eq!(ledger.unknown(&claim), Ok(()));
    assert_eq!(ledger, before);
    assert_eq!(ledger.messages[0].state, MessageState::Consumed);
}

#[test]
fn unknown_with_unmatched_claim_conflicts_without_mutation() {
    let mut ledger = fresh();
    let claim = issue(&mut ledger, 30, 31, 80);
    let mut other = claim.clone();
    other.generation += 1;
    let before = ledger.clone();
    assert_eq!(ledger.unknown(&other), Err(Error::Conflict));
    assert_eq!(ledger, before);
}

#[test]
fn any_epoch_mismatch_is_expired_before_other_witness_checks() {
    let mut ledger = fresh();
    churn(&mut ledger, 1000, 40);
    ledger.record_boundary(boundary(&ledger, 12)).unwrap();
    // Admission can itself compact, so the witness is taken afterwards.
    ledger.send(send(&ledger, 30, "q")).unwrap();
    let epoch = ledger.ledger_epoch;
    assert!(epoch > 1);
    let current = ledger.witness();
    for wrong in [epoch - 1, 0, epoch + 1, epoch + 1000] {
        let mut witness = with_epoch(&current, wrong);
        let before = ledger.clone();
        let mut intent = send(&ledger, 31, "x");
        intent.expected = witness.clone();
        assert_eq!(ledger.send(intent), Err(Error::Expired), "send {wrong}");
        let mut cancel = cancel_intent(&ledger, 32, 30);
        cancel.expected = witness.clone();
        assert_eq!(ledger.cancel(cancel), Err(Error::Expired), "cancel {wrong}");
        let mut claim = claim_intent(&ledger, 33, 81);
        claim.expected = witness.clone();
        assert_eq!(ledger.claim(claim), Err(Error::Expired), "claim {wrong}");
        let mut evidence = boundary(&ledger, 12);
        evidence.expected = witness.clone();
        assert_eq!(ledger.record_boundary(evidence), Err(Error::Expired));
        assert_eq!(ledger.close(&witness), Err(Error::Expired));
        // Epoch is evaluated before generation and turn.
        witness.generation += 5;
        assert_eq!(ledger.close(&witness), Err(Error::Expired));
        assert_eq!(ledger, before, "refusals must not mutate");
    }
}

#[test]
fn matching_epoch_with_other_witness_mismatch_is_stale() {
    let mut ledger = ready();
    let mut witness = ledger.witness();
    witness.generation += 1;
    assert_eq!(ledger.close(&witness), Err(Error::Stale));
    let mut witness = ledger.witness();
    witness.turn = id(999);
    let mut intent = claim_intent(&ledger, 33, 81);
    intent.expected = witness;
    assert_eq!(ledger.claim(intent), Err(Error::Stale));
}

#[test]
fn future_epoch_on_a_fresh_ledger_is_expired_not_stale() {
    let mut ledger = ready();
    let mut intent = send(&ledger, 31, "x");
    intent.expected = with_epoch(&ledger.witness(), 1);
    assert_eq!(ledger.send(intent), Err(Error::Expired));
}

#[test]
fn boundary_is_first_wins_by_evidence_id_only() {
    let mut ledger = fresh();
    ledger.record_boundary(boundary(&ledger, 12)).unwrap();
    let original = ledger.boundary.clone().unwrap();
    assert_eq!(ledger.record_boundary(boundary(&ledger, 12)), Ok(()));
    assert_eq!(ledger.boundary, Some(original.clone()));
    assert_eq!(
        ledger.record_boundary(boundary(&ledger, 13)),
        Err(Error::Conflict)
    );
    assert_eq!(ledger.boundary, Some(original));
}

#[test]
fn boundary_keeps_its_original_epoch_across_compaction() {
    let mut ledger = fresh();
    ledger.record_boundary(boundary(&ledger, 12)).unwrap();
    let old_witness = ledger.witness();
    assert_eq!(old_witness.ledger_epoch, 0);
    churn(&mut ledger, 1000, 40);
    assert!(ledger.ledger_epoch > 0);
    assert_eq!(ledger.boundary.as_ref().unwrap().expected, old_witness);
    // Same evidence id with the new witness is a no-op that keeps the original epoch.
    assert_eq!(ledger.record_boundary(boundary(&ledger, 12)), Ok(()));
    assert_eq!(ledger.boundary.as_ref().unwrap().expected.ledger_epoch, 0);
    assert_eq!(ledger.validate(), Ok(()));
    // Same evidence id with the old witness is Expired.
    let mut stale = boundary(&ledger, 12);
    stale.expected = old_witness;
    assert_eq!(ledger.record_boundary(stale), Err(Error::Expired));
    // Different evidence id with the new witness conflicts.
    assert_eq!(
        ledger.record_boundary(boundary(&ledger, 13)),
        Err(Error::Conflict)
    );
    assert_eq!(ledger.boundary.as_ref().unwrap().evidence, id(12));
}

#[test]
fn closed_ledger_refuses_boundary_and_acknowledge_with_closed() {
    let mut ledger = fresh();
    let claim = issue(&mut ledger, 30, 31, 80);
    consume(&mut ledger, claim.clone(), 40);
    ledger.record_boundary(boundary(&ledger, 12)).unwrap();
    assert_eq!(ledger.acknowledge(&claim), Ok(()));
    ledger.close(&ledger.witness()).unwrap();
    let before = ledger.clone();
    assert_eq!(ledger.acknowledge(&claim), Err(Error::Closed));
    assert_eq!(
        ledger.record_boundary(boundary(&ledger, 12)),
        Err(Error::Closed)
    );
    assert_eq!(
        ledger.record_boundary(boundary(&ledger, 13)),
        Err(Error::Closed)
    );
    assert_eq!(ledger, before);
}

#[test]
fn fault_requires_the_current_generation_and_turn() {
    let mut ledger = fresh();
    let (generation, turn) = (ledger.generation, ledger.turn.clone());
    let before = ledger.clone();
    assert_eq!(ledger.fault(generation + 1, &turn), Err(Error::Stale));
    assert_eq!(ledger.fault(generation, &id(999)), Err(Error::Stale));
    assert_eq!(ledger, before);
    assert!(!ledger.runtime_fault);
    assert_eq!(ledger.fault(generation, &turn), Ok(()));
    assert!(ledger.runtime_fault);
    assert_eq!(ledger.fault(generation, &turn), Ok(()));
    assert_eq!(ledger.validate(), Ok(()));
}

#[test]
fn late_fault_report_for_an_earlier_turn_cannot_fault_a_later_turn() {
    let mut ledger = fresh();
    let (old_generation, old_turn) = (ledger.generation, ledger.turn.clone());
    issue(&mut ledger, 30, 31, 80);
    assert_ne!(ledger.generation, old_generation);
    assert_eq!(ledger.fault(old_generation, &old_turn), Err(Error::Stale));
    assert!(!ledger.runtime_fault);
    assert_eq!(
        ledger.fault(ledger.generation, &ledger.turn.clone()),
        Ok(())
    );
}

#[test]
fn fault_ignores_the_ledger_epoch_after_compaction() {
    let mut ledger = fresh();
    churn(&mut ledger, 1000, 40);
    assert!(ledger.ledger_epoch > 0);
    let (generation, turn) = (ledger.generation, ledger.turn.clone());
    assert_eq!(ledger.fault(generation, &turn), Ok(()));
    assert!(ledger.runtime_fault);
}

#[test]
fn fault_on_a_closed_ledger_is_closed_and_a_mismatch_stays_stale() {
    let mut ledger = fresh();
    ledger.record_boundary(boundary(&ledger, 12)).unwrap();
    ledger.close(&ledger.witness()).unwrap();
    let (generation, turn) = (ledger.generation, ledger.turn.clone());
    assert_eq!(ledger.fault(generation, &turn), Err(Error::Closed));
    assert!(!ledger.runtime_fault);
    assert_eq!(ledger.fault(generation + 1, &turn), Err(Error::Stale));
}

#[test]
fn fault_blocks_admission_claim_boundary_and_close() {
    let mut ledger = ready();
    ledger
        .fault(ledger.generation, &ledger.turn.clone())
        .unwrap();
    assert_eq!(ledger.send(send(&ledger, 31, "x")), Err(Error::Fault));
    assert_eq!(
        ledger.claim(claim_intent(&ledger, 33, 81)),
        Err(Error::Fault)
    );
    assert_eq!(ledger.close(&ledger.witness()), Err(Error::Blocked));
    assert_eq!(
        ledger.record_boundary(boundary(&ledger, 12)),
        Err(Error::Blocked)
    );
}

#[test]
fn receipt_is_idempotent_and_a_different_evidence_conflicts() {
    let mut ledger = fresh();
    let claim = issue(&mut ledger, 30, 31, 80);
    let receipt = Receipt {
        claim: claim.clone(),
        evidence: id(40),
    };
    ledger.receipt(receipt.clone()).unwrap();
    assert_eq!(ledger.receipt(receipt), Ok(()));
    let other = Receipt {
        claim,
        evidence: id(41),
    };
    assert_eq!(ledger.receipt(other), Err(Error::Conflict));
}
