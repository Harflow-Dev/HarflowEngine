#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::support::*;
use crate::Mutation;
use hf_application::task_watch::{TaskWatchPort, WatchError};
use hf_domain::{
    task_read::TaskId,
    task_watch::{WatchReason, WatchState},
};
#[test]
fn baseline_cursor_observes_atomic_publish_and_write_revert_as_two_invalidations() {
    let fixture = Fixture::new("write-revert");
    let writer = fixture.initialize();
    let watch = watch(&fixture.root(), Clock::default());
    let (initial, cursor, reason) = state(watch.execute(&request(None, 0), deadline()).unwrap());
    assert_eq!((initial, reason), (WatchState::Baseline, None));
    let cursor = cursor.unwrap();
    let original_cursor = cursor.clone();
    publish(&writer, &[("task-a", b"first")]);
    let (changed_state, changed, reason) = state(
        watch
            .execute(&request(Some(cursor), 0), deadline())
            .unwrap(),
    );
    assert_eq!((changed_state, reason), (WatchState::Changed, None));
    let changed = changed.unwrap();
    assert_eq!(read_revision(&fixture.raw()), 1);
    assert_eq!(
        fixture
            .raw()
            .query_row(
                "SELECT header FROM task_headers WHERE id='task-a'",
                [],
                |row| row.get::<_, Vec<u8>>(0)
            )
            .unwrap(),
        b"first"
    );
    publish(&writer, &[("task-a", b"original-again")]);
    let (reverted_state, reverted, reason) = state(
        watch
            .execute(&request(Some(changed), 0), deadline())
            .unwrap(),
    );
    assert_eq!((reverted_state, reason), (WatchState::Changed, None));
    assert_eq!(read_revision(&fixture.raw()), 2);
    assert_eq!(
        fixture
            .raw()
            .query_row(
                "SELECT header FROM task_headers WHERE id='task-a'",
                [],
                |row| row.get::<_, Vec<u8>>(0)
            )
            .unwrap(),
        b"original-again"
    );
    assert_ne!(reverted, Some(original_cursor));
}

#[test]
fn identical_metadata_republication_is_a_conservative_invalidation() {
    let fixture = Fixture::new("identical-republish");
    let writer = fixture.initialize();
    publish(&writer, &[("same", b"identical")]);
    let baseline = watch(&fixture.root(), Clock::default())
        .execute(&request(None, 0), deadline())
        .unwrap()
        .cursor
        .unwrap();
    publish(&writer, &[("same", b"identical")]);
    let result = watch(&fixture.root(), Clock::default())
        .execute(&request(Some(baseline), 0), deadline())
        .unwrap();
    assert_eq!(result.state, WatchState::Changed);
    assert_eq!(read_revision(&fixture.raw()), 2);
}

#[test]
fn rapid_change_then_revert_is_changed_even_when_current_header_matches_baseline() {
    let fixture = Fixture::new("rapid-revert");
    let writer = fixture.initialize();
    publish(&writer, &[("same", b"original")]);
    let baseline = watch(&fixture.root(), Clock::default())
        .execute(&request(None, 0), deadline())
        .unwrap()
        .cursor
        .unwrap();
    publish(&writer, &[("same", b"temporary-change")]);
    publish(&writer, &[("same", b"original")]);
    let result = watch(&fixture.root(), Clock::default())
        .execute(&request(Some(baseline), 0), deadline())
        .unwrap();
    assert_eq!(result.state, WatchState::Changed);
    assert_eq!(read_revision(&fixture.raw()), 3);
    assert_eq!(
        fixture
            .raw()
            .query_row(
                "SELECT header FROM task_headers WHERE id='same'",
                [],
                |row| row.get::<_, Vec<u8>>(0)
            )
            .unwrap(),
        b"original"
    );
}

#[test]
fn writer_opened_on_another_handle_in_same_pid_is_independently_observed() {
    let fixture = Fixture::new("same-pid");
    let setup = fixture.initialize();
    drop(setup);
    let writer = fixture.open();
    let watch = watch(&fixture.root(), Clock::default());
    let baseline = watch
        .execute(&request(None, 0), deadline())
        .unwrap()
        .cursor
        .unwrap();
    publish(&writer, &[("same-process", b"published")]);
    assert_eq!(
        watch
            .execute(&request(Some(baseline), 0), deadline())
            .unwrap()
            .state,
        WatchState::Changed
    );
    assert_eq!(read_revision(&fixture.raw()), 1);
}

#[test]
fn pending_overlay_does_not_publish_and_real_watch_deadline_returns_timeout() {
    let fixture = Fixture::new("pending-only");
    let writer = fixture.initialize();
    publish(&writer, &[("task-a", b"header")]);
    let before = read_revision(&fixture.raw());
    let baseline = watch(&fixture.root(), Clock::default())
        .execute(&request(None, 0), deadline())
        .unwrap()
        .cursor
        .unwrap();
    let id = TaskId::new("pending-only".into()).unwrap();
    writer.replace_pending_fixture(&[id], deadline()).unwrap();
    writer
        .apply_batch(
            &[Mutation::Insert {
                key: "unrelated-namespace".into(),
                value: b"does-not-publish-task-headers".to_vec(),
            }],
            deadline(),
        )
        .unwrap();
    assert_eq!(read_revision(&fixture.raw()), before);
    let clock = Clock::default();
    let session = watch(&fixture.root(), clock.clone());
    assert_eq!(
        session
            .execute(&request(Some(baseline), 2), deadline())
            .unwrap()
            .state,
        WatchState::Timeout
    );
    assert_eq!(
        clock.waits(),
        40,
        "logical time advances once per 50ms poll over two seconds"
    );
    assert_eq!(read_revision(&fixture.raw()), before);
    let raw = fixture.raw();
    let unrelated: Vec<u8> = raw
        .query_row(
            "SELECT value FROM fixture_records WHERE key='unrelated-namespace'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(unrelated, b"does-not-publish-task-headers");
}

#[test]
fn admitted_publication_after_the_baseline_is_not_lost_at_wait_entry() {
    let fixture = Fixture::new("baseline-boundary");
    let writer = fixture.initialize();
    let baseline = watch(&fixture.root(), Clock::default())
        .execute(&request(None, 0), deadline())
        .unwrap()
        .cursor
        .unwrap();
    publish(&writer, &[("between", b"baseline-to-wait")]);
    let result = watch(&fixture.root(), Clock::default())
        .execute(&request(Some(baseline), 4), deadline())
        .unwrap();
    assert_eq!(result.state, WatchState::Changed);
    assert_eq!(read_revision(&fixture.raw()), 1);
}

#[test]
fn missing_root_precedes_malformed_cursor_and_does_not_initialize_anything() {
    let fixture = Fixture::new("missing");
    let absent = fixture.root();
    let result = watch(&absent, Clock::default())
        .execute(&request(Some("not-a-cursor".into()), 3), deadline())
        .unwrap();
    assert_eq!(result.state, WatchState::Unavailable);
    assert_eq!(result.reason, Some(WatchReason::StateMissing));
    assert_eq!(result.cursor, None);
    assert!(!absent.exists());
}

#[test]
fn saturated_root_claim_is_state_busy_not_a_watch_timeout() {
    let fixture = Fixture::new("state-busy");
    let store = fixture.initialize();
    drop(store);
    let baseline = watch(&fixture.root(), Clock::default())
        .execute(&request(None, 0), deadline())
        .unwrap()
        .cursor
        .unwrap();
    let claim = std::fs::File::open(fixture.root().join("store.lock")).unwrap();
    claim.try_lock().unwrap();
    assert_eq!(
        watch(&fixture.root(), Clock::default()).execute(&request(Some(baseline), 3), deadline()),
        Err(WatchError::StateBusy)
    );
}
