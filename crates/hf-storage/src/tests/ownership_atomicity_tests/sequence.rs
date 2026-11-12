//! `commit_sequence` for ownership writes: one commit per write, crash cuts, and no change
//! from observation or from a refused write.
use super::*;
use crate::tests::commit_sequence_tests::{expected, sequence};

#[test]
fn every_ownership_write_advances_the_sequence_by_one_and_observation_never_does() {
    let fixture = Fixture::new("seq-steps");
    let store = fixture.store();
    let root = fixture.root();
    assert_eq!(sequence(&root), 0);
    let req = request(&store, 2, 3, 4, 5);
    store.ownership_prepare(&req, deadline()).unwrap();
    assert_eq!(sequence(&root), 1);
    store.ownership_observe(&req.operation, deadline()).unwrap();
    assert_eq!(sequence(&root), 1, "observe is a read");
    let ClaimResult::Granted(claimed) = store
        .ownership_claim(&req.operation, &req.generation, 0, &owner(20), deadline())
        .unwrap()
    else {
        panic!("claim")
    };
    assert_eq!(sequence(&root), 2);
    let spawned = store
        .ownership_record_spawn(&claimed.fence().unwrap(), &fixture_process(808), deadline())
        .unwrap();
    assert_eq!(sequence(&root), 3);
    store
        .ownership_record_acceptance(&spawned.fence().unwrap(), deadline())
        .unwrap();
    assert_eq!(sequence(&root), 4);
    store.ownership_observe(&req.operation, deadline()).unwrap();
    assert_eq!(sequence(&root), 4);
}

#[test]
fn an_ownership_write_refused_inside_the_action_leaves_the_sequence_unchanged() {
    let fixture = Fixture::new("seq-refused");
    let store = fixture.store();
    let req = request(&store, 2, 3, 4, 5);
    store.ownership_prepare(&req, deadline()).unwrap();
    let ClaimResult::Granted(claimed) = store
        .ownership_claim(&req.operation, &req.generation, 0, &owner(20), deadline())
        .unwrap()
    else {
        panic!("claim")
    };
    let stale = claimed.fence().unwrap();
    store
        .ownership_record_spawn(&stale, &fixture_process(808), deadline())
        .unwrap();
    let before = sequence(&fixture.root());
    assert_eq!(before, 3);
    let again = store.ownership_record_spawn(&stale, &fixture_process(809), deadline());
    assert!(again.is_err(), "a stale fence is refused: {again:?}");
    assert_eq!(sequence(&fixture.root()), before);
}

#[test]
fn prepare_cut_at_each_stage_leaves_the_sequence_at_baseline_or_plus_one() {
    for hook in ["ownership_before_commit", "ownership_after_commit"] {
        let fixture = Fixture::new("seq-cut");
        let store = fixture.store();
        let req = request(&store, 2, 3, 4, 5);
        drop(store);
        let baseline = sequence(&fixture.root());
        let mut args = args_for_request(&req);
        args.push(("HF_B1_HOOK", hook.to_owned()));
        let mut actor = ActorGuard::spawn(&fixture.root(), &fixture.ledger(), "prepare", 2, &args);
        actor.go().unwrap();
        actor
            .wait_for(&format!("STAGE:{hook}"), deadline())
            .unwrap();
        assert_eq!(sequence(&fixture.root()), expected(baseline, hook));
        actor.kill_and_reap().unwrap();
        assert_eq!(
            sequence(&fixture.root()),
            expected(baseline, hook),
            "{hook}"
        );
    }
}
