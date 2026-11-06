//! Process contention. Leases are try-locks, so a contender is refused with
//! Busy immediately and never reaches SQLite. This does not qualify the
//! cross-process lock itself; that belongs to the platform crate.
use super::{
    actor::{Actor, Spec},
    fixture::{Fixture, cancel_intent, claim_intent, deadline, observe, prime},
};
use crate::{Error, tests::fixture_guard};
use hf_application::conversation::Command;
use hf_domain::conversation::{Enrollment, Error as DomainError, Ledger, MessageState};

const BEFORE: &str = "conversation_before_commit";

struct Paused {
    fixture: Fixture,
    enrollment: Enrollment,
    baseline: Vec<u8>,
    before: Ledger,
    holder: Actor,
}

/// Holder A is paused inside its transaction; contender B is ready before A
/// receives GO, because A's deadline runs while it is paused. Asserts B is
/// refused with Busy and the ledger bytes are unchanged.
fn pause(holder: &'static str, contender: &'static str) -> Paused {
    let fixture = Fixture::new("pause");
    let enrollment = prime(&fixture, 1, true);
    let baseline = fixture.ledger_bytes(&enrollment);
    let before = observe(&fixture.reopen(), &enrollment);
    // Opening takes the io lease briefly, so actors are started one at a time.
    let mut holder = Actor::spawn(&fixture, &Spec::new(holder, 0).hook(BEFORE).target(30));
    holder.ready();
    let mut other = Actor::spawn(&fixture, &Spec::new(contender, 1).target(30));
    other.ready();
    holder.go();
    holder.stage(BEFORE);
    other.go();
    assert_eq!(other.outcome(), "Err:Busy");
    other.exited();
    assert_eq!(fixture.ledger_bytes(&enrollment), baseline);
    Paused {
        fixture,
        enrollment,
        baseline,
        before,
        holder,
    }
}

impl Paused {
    fn finish(&mut self, expected: &str) {
        self.holder.release();
        assert_eq!(self.holder.outcome(), expected);
        self.holder.exited();
        assert_ne!(self.fixture.ledger_bytes(&self.enrollment), self.baseline);
    }
}

fn conversation_error<T: std::fmt::Debug>(result: Result<T, Error>) -> DomainError {
    match result {
        Err(Error::Conversation(error)) => error,
        other => panic!("expected a conversation refusal, got {other:?}"),
    }
}

#[test]
fn claim_first_makes_a_pre_claim_cancel_stale_and_a_fresh_one_already_claimed() {
    let _guard = fixture_guard();
    let mut paused = pause("claim", "cancel");
    paused.finish("Ok:grant");
    let store = paused.fixture.reopen();
    let id = &paused.enrollment.conversation;
    let stale = cancel_intent(&paused.before, 191, 30);
    let result = store.conversation_apply(id, Command::Cancel(stale), deadline());
    assert_eq!(conversation_error(result), DomainError::Stale);
    let current = observe(&store, &paused.enrollment);
    assert_eq!(current.messages[0].state, MessageState::Claimed);
    let fresh = cancel_intent(&current, 191, 30);
    let result = store.conversation_apply(id, Command::Cancel(fresh), deadline());
    assert_eq!(conversation_error(result), DomainError::AlreadyClaimed);
    assert_eq!(observe(&store, &paused.enrollment), current);
}

#[test]
fn cancel_first_leaves_a_claim_with_nothing_queued() {
    let _guard = fixture_guard();
    let mut paused = pause("cancel", "claim");
    paused.finish("Ok");
    let store = paused.fixture.reopen();
    let retry = claim_intent(&paused.before, 151, 171);
    let result = store.conversation_apply(
        &paused.enrollment.conversation,
        Command::Claim(retry),
        deadline(),
    );
    assert_eq!(conversation_error(result), DomainError::Missing);
    let current = observe(&store, &paused.enrollment);
    assert_eq!(current.messages[0].state, MessageState::Cancelled);
    assert_eq!(current.cumulative_issuances, 0);
}

#[test]
fn admit_first_leaves_the_claim_on_the_earlier_fifo_message() {
    let _guard = fixture_guard();
    let mut paused = pause("send", "claim");
    paused.finish("Ok");
    let store = paused.fixture.reopen();
    let retry = claim_intent(&paused.before, 151, 171);
    let outcome = store
        .conversation_apply(
            &paused.enrollment.conversation,
            Command::Claim(retry),
            deadline(),
        )
        .unwrap();
    let grant = outcome.grant.expect("a fresh grant");
    assert_eq!(grant.claim().message, super::fixture::id(30));
    assert_eq!(outcome.ledger.messages.len(), 2);
    assert_eq!(outcome.ledger.messages[1].state, MessageState::Queued);
}

#[test]
fn racing_claim_processes_yield_exactly_one_fresh_grant() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("race");
    let enrollment = prime(&fixture, 1, true);
    let mut actors: Vec<Actor> = Vec::new();
    for n in 0..4 {
        let actor = Actor::spawn(&fixture, &Spec::new("claim", n).retry_busy());
        actor.ready();
        actors.push(actor);
    }
    actors.iter_mut().for_each(Actor::go);
    let outcomes: Vec<String> = actors.iter().map(Actor::outcome).collect();
    actors.iter_mut().for_each(Actor::exited);
    let allowed = [
        "Ok:grant",
        "Ok:nogrant",
        "Err:Conversation(Blocked)",
        "Err:Conversation(Stale)",
    ];
    for outcome in &outcomes {
        assert!(allowed.contains(&outcome.as_str()), "outcome {outcome}");
    }
    let grants = outcomes.iter().filter(|o| *o == "Ok:grant").count();
    assert_eq!(grants, 1, "outcomes: {outcomes:?}");
    let ledger = observe(&fixture.reopen(), &enrollment);
    assert_eq!(ledger.cumulative_issuances, 1);
    assert_eq!(ledger.messages[0].state, MessageState::Claimed);
}
