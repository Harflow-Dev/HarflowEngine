//! `commit_sequence` at the capture crash stages, including the terminal commit.
use super::support::{Actor, Fixture};
use crate::tests::commit_sequence_tests::{expected, sequence};

#[test]
fn capture_cuts_leave_the_sequence_at_baseline_or_plus_one_for_progress_and_terminal() {
    for (action, hook) in [
        ("progress", "capture_before_commit"),
        ("progress", "capture_after_commit"),
        ("finish", "capture_before_terminal"),
        ("finish", "capture_after_terminal"),
    ] {
        let fixture = Fixture::new();
        let mut actor = Actor::spawn(&fixture, action, hook);
        let baseline = sequence(&fixture.root());
        assert!(baseline > 0, "priming committed through the same path");
        actor.go();
        actor.line(&format!("STAGE:{hook}"));
        assert_eq!(
            sequence(&fixture.root()),
            expected(baseline, hook),
            "{action} at {hook}"
        );
        actor.kill_and_reap();
        assert_eq!(
            sequence(&fixture.root()),
            expected(baseline, hook),
            "after the kill at {hook}"
        );
    }
}
