#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! A failure of the whole command, reported by the target port after the page is observed.
mod task_artifacts_support;
mod task_reads_support;
use hf_application::ctl::task_reads::{ArtifactCursorArg, artifacts};
use task_artifacts_support::*;
use task_reads_support::{NONE_MARKERS, root};

const MESSAGE: &str = "'getpwuid(): uid not found: 4242'";

#[test]
fn a_command_failure_ends_the_read_as_an_artifact_error_with_its_message() {
    let world = world(3);
    let mut found = targets();
    found.failure = Some(MESSAGE.into());
    let outcome = artifacts(
        &world.reads(NONE_MARKERS),
        &found,
        &root(),
        &request(20, ArtifactCursorArg::Absent),
        &|_, _| 0,
    );
    assert_eq!(outcome, refused(MESSAGE));
    assert_eq!(found.seen.borrow().len(), 3, "the page was observed first");
}

#[test]
fn without_a_failure_the_page_is_returned() {
    let world = world(3);
    let found = targets();
    let page = artifacts(
        &world.reads(NONE_MARKERS),
        &found,
        &root(),
        &request(20, ArtifactCursorArg::Absent),
        &|_, _| 0,
    )
    .unwrap();
    assert_eq!(page.entries.len(), 3);
}

#[test]
fn a_refusal_before_the_page_is_reported_as_it_was_and_asks_nothing_of_the_port() {
    let world = world(3);
    let mut found = targets();
    found.failure = Some(MESSAGE.into());
    let mut bad = request(0, ArtifactCursorArg::Absent);
    bad.limit = 0;
    let outcome = artifacts(&world.reads(NONE_MARKERS), &found, &root(), &bad, &|_, _| 0);
    assert_eq!(outcome, refused("Artifact limit must be between 1 and 50"));
    assert!(found.seen.borrow().is_empty());
}
