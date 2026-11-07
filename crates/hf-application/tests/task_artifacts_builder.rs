#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The workflow-builder refusal of the artifact read, and the failures a brief status keeps.
mod task_artifacts_support;
mod task_reads_support;
use hf_application::ctl::task_reads::{ArtifactCursorArg, ReadError, StoreFailure, status_with};
use hf_domain::ctl::{history::ArtifactSummary, inspect::InspectReason};
use task_artifacts_support::*;
use task_reads_support::{NONE_MARKERS, World, record, root, task_id};

const MESSAGE: &str = "Managed workflow artifact access is not supported";

fn chain(builders: &[u32]) -> World {
    let mut world = World::new();
    world.history.noisy = true;
    for number in 1..=3 {
        let mut task = record(number, "completed", Some(0), None);
        task.facts.continuation_of = (number > 1).then(|| task_id(number - 1));
        world.tasks.push(task);
    }
    world.history.builders = builders
        .iter()
        .map(|n| task_id(*n).as_str().to_string())
        .collect();
    world
}

fn tip() -> hf_application::ctl::task_reads::ArtifactRequest {
    let mut asked = request(20, ArtifactCursorArg::Absent);
    asked.task_id = task_id(3).as_str().to_string();
    asked
}

#[test]
fn a_builder_anywhere_in_the_chain_refuses_before_the_manifest_is_read() {
    for builders in [vec![3], vec![2], vec![1], vec![1, 2, 3]] {
        let world = chain(&builders);
        let outcome = read(&world, NONE_MARKERS, &tip());
        assert_eq!(outcome, refused(MESSAGE), "{builders:?}");
        assert_eq!(world.calls("manifest"), 0, "{builders:?}");
    }
}

#[test]
fn no_builder_in_the_chain_reads_normally() {
    let world = chain(&[]);
    assert!(read(&world, NONE_MARKERS, &tip()).is_ok());
    let unrelated = chain(&[9]);
    assert!(read(&unrelated, NONE_MARKERS, &tip()).is_ok());
}

#[test]
fn a_builder_makes_the_summary_unavailable_with_no_detail_reference() {
    for builders in [vec![3], vec![1]] {
        let world = chain(&builders);
        let out = status_with(
            &world.reads(NONE_MARKERS),
            &root(),
            task_id(3).as_str(),
            false,
        )
        .unwrap();
        assert_eq!(
            out.artifacts,
            ArtifactSummary {
                count: 0,
                available: false,
                detail_ref: false
            }
        );
        assert_eq!(world.calls("manifest"), 0);
    }
}

#[test]
fn a_broken_link_is_reported_before_a_later_builder_is_found() {
    let mut world = chain(&[1]);
    world.tasks[1].facts.continuation_of = Some(task_id(900));
    assert_eq!(
        read(&world, NONE_MARKERS, &tip()),
        refused("Task ancestry is unavailable")
    );
}

#[test]
fn store_busy_and_incompatible_failures_stay_fatal_under_brief() {
    let incompatible = StoreFailure::Incompatible {
        reason: InspectReason::InfrastructureUnavailable,
        message: "Task history cannot be read".into(),
    };
    for failure in [StoreFailure::Busy, incompatible] {
        let mut world = chain(&[]);
        world.history.recent_error = Some(failure.clone());
        let id = task_id(3);
        let brief = status_with(&world.reads(NONE_MARKERS), &root(), id.as_str(), true);
        assert_eq!(brief.err(), Some(ReadError::Store(failure.clone())));
        assert!(status_with(&world.reads(NONE_MARKERS), &root(), id.as_str(), false).is_ok());
    }
}
