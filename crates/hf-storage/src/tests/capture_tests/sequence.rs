//! `commit_sequence` for capture writes: one commit per begin, publish and finish.
use super::support::{Fixture, accepted_binding, deadline};
use crate::tests::commit_sequence_tests::sequence;
use hf_domain::{capture::CaptureProgress, process::ProcessExit};

fn done(revision: u64) -> CaptureProgress {
    CaptureProgress {
        revision,
        stdout: 1,
        stderr: 1,
        stdout_eof: true,
        stderr_eof: true,
        loss: false,
    }
}

#[test]
fn begin_publish_and_finish_each_advance_the_sequence_by_one_and_observe_does_not() {
    let fixture = Fixture::new("seq-steps");
    let store = fixture.store();
    let binding = accepted_binding(&store, 1);
    let root = fixture.root();
    let accepted = sequence(&root);
    assert_eq!(accepted, 4);
    store.capture_begin(&binding, deadline()).unwrap();
    assert_eq!(sequence(&root), accepted + 1);
    store
        .capture_publish(&binding, 0, &done(1), deadline())
        .unwrap();
    assert_eq!(sequence(&root), accepted + 2);
    store
        .capture_observe(&binding.fence.operation, deadline())
        .unwrap();
    assert_eq!(sequence(&root), accepted + 2, "observe is a read");
    store
        .capture_finish(&binding, 1, ProcessExit::Code(0), deadline())
        .unwrap();
    assert_eq!(sequence(&root), accepted + 3, "terminal commit counts once");
}

#[test]
fn a_publish_with_a_stale_revision_leaves_the_sequence_unchanged() {
    let fixture = Fixture::new("seq-refused");
    let store = fixture.store();
    let binding = accepted_binding(&store, 1);
    store.capture_begin(&binding, deadline()).unwrap();
    let before = sequence(&fixture.root());
    let refused = store.capture_publish(&binding, 7, &done(8), deadline());
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(sequence(&fixture.root()), before);
}
