#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The continuation stop that `status`, `status --brief` and `status-batch` carry, read with
//! the record in the same snapshot.
mod task_reads_support;

use hf_application::ctl::task_reads::{ReadError, status, status_batch, status_with};
use hf_domain::ctl::{
    continuation_stop::{ContinuationStop, StopRow, StopState},
    task_rows::ChainRoot,
};
use task_reads_support::{NONE_MARKERS, World, record, root, task_id};

fn world() -> World {
    let mut world = World::new();
    world.tasks.push(record(1, "completed", Some(0), None));
    world.tasks.push(record(2, "completed", Some(0), None));
    world.tasks[1].facts.continuation_of = Some(task_id(1));
    world.tasks[1].facts.chain_root = ChainRoot::Valid(task_id(1));
    world
}

fn stopped_by(n: u32) -> StopRow {
    StopRow::Marker {
        attribution: Some(task_id(n).as_str().to_owned()),
    }
}

#[test]
fn a_task_with_no_row_is_not_stopped_at_its_own_id() {
    let out = status(&world().reads(NONE_MARKERS), &root(), task_id(1).as_str()).unwrap();
    assert_eq!(out.stop.state, StopState::NotStopped);
    assert_eq!(out.stop.root, Some(task_id(1)));
    assert_eq!(out.stop.cancelled, None);
}

#[test]
fn status_serves_the_row_projected_through_the_domain() {
    let mut world = world();
    world.stops.push((task_id(2), stopped_by(2)));
    let out = status(&world.reads(NONE_MARKERS), &root(), task_id(2).as_str()).unwrap();
    assert_eq!(
        out.stop,
        ContinuationStop::observe(&task_id(2), &ChainRoot::Valid(task_id(1)), &stopped_by(2))
    );
    assert_eq!(out.stop.state, StopState::Stopped);
    assert_eq!(out.stop.root, Some(task_id(1)));
    assert_eq!(out.stop.cancelled, Some(task_id(2)));
}

#[test]
fn a_fault_row_is_unknown_and_an_invalid_stored_root_ignores_the_row() {
    let mut world = world();
    world.stops.push((task_id(1), StopRow::Fault));
    world.stops.push((task_id(2), stopped_by(1)));
    world.tasks[1].facts.chain_root = ChainRoot::Invalid("not a valid id!".into());
    let reads = world.reads(NONE_MARKERS);
    let fault = status(&reads, &root(), task_id(1).as_str()).unwrap().stop;
    assert_eq!(
        (fault.state, fault.root),
        (StopState::Unknown, Some(task_id(1)))
    );
    let invalid = status(&reads, &root(), task_id(2).as_str()).unwrap().stop;
    assert_eq!(
        (invalid.state, invalid.root, invalid.cancelled),
        (StopState::Unknown, None, None)
    );
}

#[test]
fn the_brief_read_carries_the_same_stop() {
    let mut world = world();
    world.stops.push((task_id(2), stopped_by(1)));
    let reads = world.reads(NONE_MARKERS);
    let plain = status_with(&reads, &root(), task_id(2).as_str(), false).unwrap();
    let brief = status_with(&reads, &root(), task_id(2).as_str(), true).unwrap();
    assert_eq!(plain.stop, brief.stop);
    assert_eq!(brief.stop.state, StopState::Stopped);
}

#[test]
fn every_batch_row_carries_its_own_stop_from_the_same_snapshot() {
    let mut world = world();
    world.stops.push((task_id(2), stopped_by(1)));
    let ids = [
        task_id(1).as_str().to_owned(),
        task_id(2).as_str().to_owned(),
    ];
    let out = status_batch(&world.reads(NONE_MARKERS), &root(), &ids).unwrap();
    // One snapshot serves the whole batch.
    assert_eq!(world.calls("snapshot"), 1);
    let states: Vec<StopState> = out.rows.iter().map(|row| row.stop.state).collect();
    assert_eq!(states, [StopState::NotStopped, StopState::Stopped]);
    assert_eq!(out.rows[1].stop.cancelled, Some(task_id(1)));
    // A single status of the same task agrees with its batch row.
    let single = status(&world.reads(NONE_MARKERS), &root(), task_id(2).as_str()).unwrap();
    assert_eq!(single.stop, out.rows[1].stop);
}

#[test]
fn the_stop_is_read_by_the_record_statement_not_a_second_lookup() {
    let world = world();
    let ids = [task_id(1).as_str().to_owned()];
    status_batch(&world.reads(NONE_MARKERS), &root(), &ids).unwrap();
    assert_eq!(world.calls("record_stop:"), 1);
    assert_eq!(world.calls("record:"), 0);
}

#[test]
fn a_vanished_record_is_still_drift_with_a_stop_row_configured() {
    let mut world = world();
    world.vanish.push(task_id(1));
    world.stops.push((task_id(1), StopRow::Fault));
    let out = status(&world.reads(NONE_MARKERS), &root(), task_id(1).as_str());
    assert!(matches!(out, Err(ReadError::Store(_))), "{out:?}");
}
