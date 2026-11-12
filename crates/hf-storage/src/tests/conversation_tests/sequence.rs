//! `commit_sequence` for conversation writes: one commit per write, no change from
//! observation or a refused write, crash cuts, and the post-commit deadline outcome.
use super::{
    actor::{Actor, Spec},
    fixture::{
        Fixture, apply, claim_intent, deadline, evidence, observe, prime, seed, send_intent,
    },
};
use crate::{
    Error,
    tests::{
        commit_sequence_tests::{expected, sequence},
        fixture_guard,
    },
};
use hf_application::conversation::Command;

const BEFORE: &str = "conversation_before_commit";
const AFTER: &str = "conversation_after_commit";

#[test]
fn enroll_send_boundary_and_claim_advance_the_sequence_by_one_each_and_observe_does_not() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("seq-steps");
    let store = fixture.store();
    let enrollment = seed(&store);
    let seeded = sequence(&fixture.root());
    assert_eq!(seeded, 4, "prepare, claim, spawn, acceptance");
    store.conversation_enroll(&enrollment, deadline()).unwrap();
    assert_eq!(sequence(&fixture.root()), seeded + 1);
    let ledger = observe(&store, &enrollment);
    assert_eq!(sequence(&fixture.root()), seeded + 1, "observe is a read");
    apply(
        &store,
        &enrollment,
        Command::Send(send_intent(&ledger, 30, "m")),
    );
    assert_eq!(sequence(&fixture.root()), seeded + 2);
    let ledger = observe(&store, &enrollment);
    apply(
        &store,
        &enrollment,
        Command::Boundary(evidence(&ledger, 12)),
    );
    assert_eq!(sequence(&fixture.root()), seeded + 3);
    let ledger = observe(&store, &enrollment);
    apply(
        &store,
        &enrollment,
        Command::Claim(claim_intent(&ledger, 150, 170)),
    );
    assert_eq!(sequence(&fixture.root()), seeded + 4);
}

#[test]
fn a_command_refused_inside_the_action_leaves_the_sequence_unchanged() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("seq-refused");
    let enrollment = prime(&fixture, 1, true);
    let store = fixture.reopen();
    let ledger = observe(&store, &enrollment);
    apply(
        &store,
        &enrollment,
        Command::Claim(claim_intent(&ledger, 150, 170)),
    );
    let before = sequence(&fixture.root());
    // The witness is now stale, so the same intent is refused.
    let refused = store.conversation_apply(
        &enrollment.conversation,
        Command::Claim(claim_intent(&ledger, 152, 172)),
        deadline(),
    );
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(sequence(&fixture.root()), before);
}

#[test]
fn send_and_claim_cuts_leave_the_sequence_at_baseline_or_plus_one() {
    let _guard = fixture_guard();
    for (op, hook) in [
        ("send", BEFORE),
        ("send", AFTER),
        ("claim", BEFORE),
        ("claim", AFTER),
    ] {
        let fixture = Fixture::new(op);
        prime(&fixture, 1, true);
        let baseline = sequence(&fixture.root());
        let mut actor = Actor::spawn(&fixture, &Spec::new(op, 0).hook(hook));
        actor.ready();
        actor.go();
        actor.stage(hook);
        assert_eq!(sequence(&fixture.root()), expected(baseline, hook), "{op}");
        actor.kill();
        assert_eq!(
            sequence(&fixture.root()),
            expected(baseline, hook),
            "{op} after the kill at {hook}"
        );
    }
}

#[test]
fn observe_in_a_child_never_changes_the_sequence() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("seq-observe");
    prime(&fixture, 1, true);
    let baseline = sequence(&fixture.root());
    let mut actor = Actor::spawn(&fixture, &Spec::new("observe", 0));
    actor.ready();
    actor.go();
    assert_eq!(actor.outcome(), "Ok");
    actor.exited();
    assert_eq!(sequence(&fixture.root()), baseline);
}

#[test]
fn deadline_expiry_after_commit_reports_unknown_outcome_with_the_commit_counted_once() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("seq-deadline");
    prime(&fixture, 1, true);
    let baseline = sequence(&fixture.root());
    let spec = Spec::new("claim", 0).hook(AFTER).deadline_ms(4000);
    let mut actor = Actor::spawn(&fixture, &spec);
    actor.ready();
    actor.go();
    actor.stage(AFTER);
    actor.wait_past_own_deadline();
    actor.release();
    assert_eq!(
        actor.outcome(),
        format!("Err:{:?}", Error::CommitOutcomeUnknown)
    );
    actor.exited();
    assert_eq!(sequence(&fixture.root()), baseline + 1);
}
