//! `commit_sequence` at the task-catalog crash stages: a pre-commit kill leaves the value
//! unchanged, a post-commit kill leaves it exactly one ahead.
use super::{Actor, Fixture, deadline};
use crate::{
    Limits, Store,
    tests::commit_sequence_tests::{expected, sequence},
};

#[test]
fn catalog_publish_cut_at_each_stage_leaves_the_sequence_at_baseline_or_plus_one() {
    for hook in ["before_task_catalog_commit", "after_task_catalog_commit"] {
        let fixture = Fixture::new();
        drop(
            Store::initialize_fixture(
                fixture.root(),
                Limits::default(),
                deadline(),
                crate::tests::lease::lease_port(),
            )
            .unwrap(),
        );
        let baseline = sequence(&fixture.root());
        assert_eq!(baseline, 0, "initialize creates the row at zero");
        let mut actor = Actor::spawn(&fixture.root(), Some(hook), &fixture.ledger());
        actor.send("PUBLISH_A");
        actor.expect(&format!("STAGE:{hook}"));
        assert_eq!(
            sequence(&fixture.root()),
            expected(baseline, hook),
            "{hook}"
        );
        actor.terminate();
        assert_eq!(
            sequence(&fixture.root()),
            expected(baseline, hook),
            "after the kill at {hook}"
        );
    }
}

#[test]
fn a_busy_second_publisher_does_not_advance_the_sequence() {
    let fixture = Fixture::new();
    drop(
        Store::initialize_fixture(
            fixture.root(),
            Limits::default(),
            deadline(),
            crate::tests::lease::lease_port(),
        )
        .unwrap(),
    );
    let mut first = Actor::spawn(
        &fixture.root(),
        Some("before_task_catalog_commit"),
        &fixture.ledger(),
    );
    let mut second = Actor::spawn(&fixture.root(), None, &fixture.ledger());
    first.send("PUBLISH_A");
    first.expect("STAGE:before_task_catalog_commit");
    second.send("PUBLISH_B");
    second.expect("RESULT:BUSY");
    assert_eq!(sequence(&fixture.root()), 0);
    first.terminate();
    second.send("PUBLISH_B");
    second.expect("RESULT:COMMITTED");
    assert_eq!(sequence(&fixture.root()), 1);
    second.send("STOP");
    second.terminate();
}
