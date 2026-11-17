#![cfg(unix)]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;
mod fixture_support;

use fixture_support::{A, done, identity, later, task, world};
use hf_storage::{Error, FixtureProcess};

fn attach_process(mut row: hf_storage::FixtureTask, pid: u32) -> hf_storage::FixtureTask {
    let process = identity(pid);
    row.process = Some(FixtureProcess {
        pid: process.pid(),
        start: Some(process),
    });
    row
}

#[test]
fn returns_full_identity_for_every_active_fact_and_excludes_only_known_terminal() {
    let world = world();
    world.add(&attach_process(task(A, "running", None, false), 9101));
    world.add(&done("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"));

    let candidates = world
        .store
        .ordinary_runtime_mutation_candidates(64, later(5))
        .unwrap();

    assert_eq!(candidates, vec![identity(9101)]);
}

#[test]
fn missing_identity_and_incomplete_coverage_refuse_the_whole_inventory() {
    let missing_process_world = world();
    missing_process_world.add(&task(A, "running", None, false));
    assert!(
        missing_process_world
            .store
            .ordinary_runtime_mutation_candidates(64, later(5))
            .is_err()
    );

    let world = world();
    world
        .store
        .replace_task_fixture(
            &hf_storage::TaskFixture {
                rows: vec![],
                launches: vec![],
                coverage: hf_domain::task_read::Coverage {
                    discovery_complete: false,
                    ownership_complete: true,
                    source_complete: true,
                },
            },
            later(5),
        )
        .unwrap();
    assert!(matches!(
        world
            .store
            .ordinary_runtime_mutation_candidates(64, later(5)),
        Err(Error::MalformedRow)
    ));
}

#[test]
fn limit_is_bounded_and_overbudget_result_is_not_partial() {
    let world = world();
    for n in 0..65 {
        let id = format!("00000000-0000-4000-8000-{n:012x}");
        world.add(&attach_process(task(&id, "running", None, false), 9200 + n));
    }
    assert!(matches!(
        world
            .store
            .ordinary_runtime_mutation_candidates(64, later(5)),
        Err(Error::LimitExceeded)
    ));
    assert!(matches!(
        world
            .store
            .ordinary_runtime_mutation_candidates(65, later(5)),
        Err(Error::LimitExceeded)
    ));
}
