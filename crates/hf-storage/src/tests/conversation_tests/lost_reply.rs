//! A committed claim whose reply never reached the caller.
use super::{
    actor::{Actor, Spec, child_claim},
    fixture::{Fixture, claim_intent, deadline, evidence, id, observe, prime},
};
use crate::{Error, Store, tests::fixture_guard};
use hf_application::conversation::Command;
use hf_domain::conversation::{Enrollment, Error as DomainError, Ledger, MessageState};

const AFTER: &str = "conversation_after_commit";

struct Lost {
    fixture: Fixture,
    enrollment: Enrollment,
    before: Ledger,
}

fn committed_claim(deadline_ms: u64, expect_outcome: Option<&str>) -> Lost {
    let fixture = Fixture::new("lost");
    let enrollment = prime(&fixture, 1, true);
    let before = observe(&fixture.reopen(), &enrollment);
    let spec = Spec::new("claim", 0).hook(AFTER).deadline_ms(deadline_ms);
    let mut actor = Actor::spawn(&fixture, &spec);
    actor.ready();
    actor.go();
    actor.stage(AFTER);
    match expect_outcome {
        None => actor.kill(),
        Some(expected) => {
            actor.wait_past_own_deadline();
            actor.release();
            assert_eq!(actor.outcome(), expected);
            actor.exited();
        }
    }
    Lost {
        fixture,
        enrollment,
        before,
    }
}

fn assert_replay_is_quiet(lost: &Lost, store: &Store) {
    let committed = observe(store, &lost.enrollment);
    assert_eq!(committed.messages[0].state, MessageState::Claimed);
    assert_eq!(committed.cumulative_issuances, 1);
    let bytes = lost.fixture.ledger_bytes(&lost.enrollment);
    let replay = child_claim(&lost.before, 0);
    let outcome = store
        .conversation_apply(
            &lost.enrollment.conversation,
            Command::Claim(replay),
            deadline(),
        )
        .unwrap();
    assert!(outcome.grant.is_none(), "exact replay must not grant again");
    assert_eq!(outcome.ledger, committed);
    assert_eq!(outcome.ledger.cumulative_issuances, 1);
    assert_eq!(lost.fixture.ledger_bytes(&lost.enrollment), bytes);
}

fn refusal(
    result: Result<hf_domain::conversation::ClaimOutcome<crate::FreshGrant>, Error>,
) -> DomainError {
    match result {
        Err(Error::Conversation(error)) => error,
        other => panic!("expected a conversation refusal, got {other:?}"),
    }
}

#[test]
fn killed_after_commit_replays_without_a_second_grant() {
    let _guard = fixture_guard();
    let lost = committed_claim(8000, None);
    let store = lost.fixture.reopen();
    assert_replay_is_quiet(&lost, &store);
    let current = observe(&store, &lost.enrollment);
    let id_of = &lost.enrollment.conversation;
    // The open claim cleared the boundary; recording one is blocked while the
    // claim is open, and a boundary forced in by hand still blocks a new claim.
    let result =
        store.conversation_apply(id_of, Command::Boundary(evidence(&current, 12)), deadline());
    assert_eq!(refusal(result), DomainError::Blocked);
    let mut forced = current.clone();
    forced.boundary = Some(evidence(&current, 12));
    let attempt = forced.claim(claim_intent(&current, 151, 171));
    assert_eq!(attempt, Err(DomainError::Blocked));
    // The same new UUID with the pre-kill witness is stale.
    let stale = claim_intent(&lost.before, 152, 172);
    let result = store.conversation_apply(id_of, Command::Claim(stale), deadline());
    assert_eq!(refusal(result), DomainError::Stale);
    assert_eq!(observe(&store, &lost.enrollment), current);
    assert_eq!(current.turn, id(170));
}

#[test]
fn deadline_expiry_after_commit_reports_unknown_outcome_and_replays_quietly() {
    let _guard = fixture_guard();
    let lost = committed_claim(4000, Some("Err:CommitOutcomeUnknown"));
    let store = lost.fixture.reopen();
    assert_replay_is_quiet(&lost, &store);
}
