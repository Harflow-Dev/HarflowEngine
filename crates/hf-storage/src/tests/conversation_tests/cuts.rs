//! Kill-at-hook cuts for send and claim, checked with a read-only SQLite
//! connection and a reopen. The stage line is printed only inside a mutation,
//! so reaching it proves the mutation under test is the one that was cut.
use super::{
    actor::{Actor, Spec},
    fixture::{Fixture, deadline, id, prime},
};
use crate::tests::fixture_guard;
use hf_domain::conversation::{Ledger, MessageState};

const BEFORE: &str = "conversation_before_commit";
const AFTER: &str = "conversation_after_commit";

struct Cut {
    baseline: Vec<u8>,
    at_stage: Vec<u8>,
    reopened: Ledger,
    generation: u64,
}

fn cut(op: &'static str, hook: &'static str) -> Cut {
    let fixture = Fixture::new(op);
    let enrollment = prime(&fixture, 1, true);
    let baseline = fixture.ledger_bytes(&enrollment);
    let mut actor = Actor::spawn(&fixture, &Spec::new(op, 0).hook(hook));
    actor.ready();
    actor.go();
    actor.stage(hook);
    let at_stage = fixture.ledger_bytes(&enrollment);
    actor.kill();
    let reopened = fixture
        .reopen()
        .conversation_observe(&enrollment.conversation, deadline())
        .unwrap();
    Cut {
        baseline,
        at_stage,
        reopened,
        generation: enrollment.generation,
    }
}

#[test]
fn send_cut_before_commit_leaves_the_ledger_unchanged() {
    let _guard = fixture_guard();
    let cut = cut("send", BEFORE);
    assert_eq!(cut.at_stage, cut.baseline);
    assert_eq!(cut.reopened.messages.len(), 1);
    assert_eq!(cut.reopened.messages[0].intent.request, id(30));
}

#[test]
fn send_cut_after_commit_is_visible_to_the_oracle_and_after_reopen() {
    let _guard = fixture_guard();
    let cut = cut("send", AFTER);
    assert_ne!(cut.at_stage, cut.baseline);
    assert_eq!(cut.reopened.messages.len(), 2);
    assert_eq!(cut.reopened.messages[1].intent.request, id(130));
    assert_eq!(cut.reopened.messages[1].state, MessageState::Queued);
}

#[test]
fn claim_cut_before_commit_leaves_the_message_queued() {
    let _guard = fixture_guard();
    let cut = cut("claim", BEFORE);
    assert_eq!(cut.at_stage, cut.baseline);
    assert_eq!(cut.reopened.messages[0].state, MessageState::Queued);
    assert_eq!(cut.reopened.generation, cut.generation);
    assert_eq!(cut.reopened.cumulative_issuances, 0);
    assert!(cut.reopened.boundary.is_some());
}

#[test]
fn claim_cut_after_commit_persists_the_claim_without_a_grant_holder() {
    let _guard = fixture_guard();
    let cut = cut("claim", AFTER);
    assert_ne!(cut.at_stage, cut.baseline);
    let message = &cut.reopened.messages[0];
    assert_eq!(message.state, MessageState::Claimed);
    assert_eq!(message.claim.as_ref().unwrap().intent.request, id(150));
    assert_eq!(cut.reopened.generation, cut.generation + 1);
    assert_eq!(cut.reopened.turn, id(170));
    assert_eq!(cut.reopened.cumulative_issuances, 1);
}

#[test]
fn observe_does_not_reach_commit_hooks() {
    let _guard = fixture_guard();
    for hook in [BEFORE, AFTER] {
        let fixture = Fixture::new("observe");
        let enrollment = prime(&fixture, 1, true);
        let baseline = fixture.ledger_bytes(&enrollment);
        let mut actor = Actor::spawn(&fixture, &Spec::new("observe", 0).hook(hook));
        actor.ready();
        actor.go();
        // A hook reached during a read would block the child before it reports.
        assert_eq!(actor.outcome(), "Ok");
        actor.exited();
        assert_eq!(fixture.ledger_bytes(&enrollment), baseline);
    }
}
