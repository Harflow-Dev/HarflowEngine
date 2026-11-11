#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The caller gate of the task reads: verdicts and candidate families.
mod task_reads_support;

use hf_application::ctl::task_reads::{ViewFailure, list, status};
use hf_domain::ctl::caller::{PidObservation, ProcessView, RefusalKind};
use task_reads_support::{
    NONE_MARKERS, World, caller,
    gate::{ALL, Path, refused, run, stale, world},
    identity, record, root, task_id,
};

#[test]
fn a_related_row_beside_an_alive_process_is_an_agent_on_every_path() {
    let mut world = world();
    world.callers = vec![caller(9, 400)];
    world
        .seen
        .insert(400, PidObservation::Present(identity(400)));
    for path in ALL {
        assert_eq!(
            run(&world, NONE_MARKERS, path),
            refused(RefusalKind::Agent),
            "{path:?}"
        );
    }
}

#[test]
fn dead_verdicts_admit_every_path() {
    for seen in [PidObservation::Absent, PidObservation::Present(stale(400))] {
        let mut world = world();
        world.callers = vec![caller(9, 400)];
        world.seen.insert(400, seen);
        for path in ALL {
            assert_eq!(run(&world, NONE_MARKERS, path), Ok(()), "{path:?}");
        }
    }
}

#[test]
fn unreadable_zombie_and_identityless_records_are_undecidable() {
    let mut blank = caller(9, 400);
    blank.process.start = None;
    for (row, seen) in [
        (caller(9, 400), PidObservation::Unknown),
        (caller(9, 400), PidObservation::Zombie),
        (blank, PidObservation::Absent),
    ] {
        let mut world = world();
        world.callers = vec![row];
        world.seen.insert(400, seen);
        for path in ALL {
            assert_eq!(
                run(&world, NONE_MARKERS, path),
                refused(RefusalKind::Undecidable),
                "{path:?}"
            );
        }
    }
}

#[test]
fn two_records_on_one_pid_each_get_their_own_verdict_in_both_orders() {
    let mut dead = caller(1, 400);
    dead.process.start = Some(stale(400));
    let live = caller(2, 400);
    let mut blank = caller(3, 400);
    blank.process.start = None;
    let cases = [
        (vec![dead.clone(), live.clone()], RefusalKind::Agent),
        (vec![live.clone(), dead.clone()], RefusalKind::Agent),
        (vec![blank.clone(), live.clone()], RefusalKind::Agent),
        (vec![live.clone(), blank.clone()], RefusalKind::Agent),
        (vec![dead.clone(), blank.clone()], RefusalKind::Undecidable),
        (vec![blank, dead], RefusalKind::Undecidable),
    ];
    for (rows, wanted) in cases {
        let mut world = world();
        world.callers = rows;
        world
            .seen
            .insert(400, PidObservation::Present(identity(400)));
        assert_eq!(run(&world, NONE_MARKERS, Path::Successors), refused(wanted));
        assert_eq!(world.calls("observe:400"), 1, "one raw observation per pid");
    }
}

#[test]
fn an_unreadable_ancestor_is_undecidable_unless_another_record_is_alive() {
    let mut world = world();
    world.callers = vec![caller(8, 300)];
    world.seen.insert(300, PidObservation::Unknown);
    assert_eq!(
        run(&world, NONE_MARKERS, Path::Successors),
        refused(RefusalKind::Undecidable)
    );
    world.callers.push(caller(9, 400));
    world
        .seen
        .insert(400, PidObservation::Present(identity(400)));
    assert_eq!(
        run(&world, NONE_MARKERS, Path::Successors),
        refused(RefusalKind::Agent)
    );
}

#[test]
fn a_failure_to_look_is_undecidable_even_without_candidates() {
    let mut world = world();
    world.view = Err(ViewFailure);
    for path in ALL {
        assert_eq!(
            run(&world, NONE_MARKERS, path),
            refused(RefusalKind::Undecidable),
            "{path:?}"
        );
    }
    assert_eq!(
        world.calls("related"),
        0,
        "no candidate scan without a view"
    );
}

#[test]
fn observed_terminal_records_are_dead_without_an_identity_check() {
    let mut world = World::new();
    world.tasks.push(record(1, "completed", Some(0), Some(400)));
    world.tasks.push(record(2, "failed", Some(1), Some(401)));
    let reads = world.reads(NONE_MARKERS);
    let one = status(&reads, &root(), task_id(1).as_str()).unwrap();
    assert_eq!(one.view.projection.identity_state, "dead");
    let all = list(&reads, &root(), None).unwrap();
    assert!(
        all.iter()
            .all(|view| view.projection.identity_state == "dead")
    );
    assert_eq!(world.calls("observe"), 0);
}

#[test]
fn the_bounded_family_reads_the_first_hundred_ancestors_and_at_most_a_hundred_rows() {
    let ancestors: Vec<u32> = (1_000..1_150).collect();
    for (count, wanted) in [(100, Ok(())), (101, refused(RefusalKind::Undecidable))] {
        for path in [Path::Plain, Path::Ids] {
            let mut world = world();
            world.view = Ok(ProcessView {
                pid: 500,
                session: 77,
                ancestors: ancestors.clone(),
            });
            world.callers = (0..count).map(|n| caller(100 + n, 5_000 + n)).collect();
            assert_eq!(run(&world, NONE_MARKERS, path), wanted, "{count} {path:?}");
            assert_eq!(world.calls("related:77:100:100:1000-1099"), 1, "{path:?}");
        }
    }
}

#[test]
fn the_complete_family_has_no_cap() {
    let ancestors: Vec<u32> = (1_000..1_150).collect();
    for path in [Path::Status, Path::Batch, Path::List, Path::Successors] {
        let mut world = world();
        world.view = Ok(ProcessView {
            pid: 500,
            session: 77,
            ancestors: ancestors.clone(),
        });
        world.callers = (0..101).map(|n| caller(100 + n, 5_000 + n)).collect();
        assert_eq!(run(&world, NONE_MARKERS, path), Ok(()), "{path:?}");
        assert_eq!(world.calls("related:77:150:none:1000-1149"), 1, "{path:?}");
        world
            .seen
            .insert(5_100, PidObservation::Present(identity(5_100)));
        assert_eq!(
            run(&world, NONE_MARKERS, path),
            refused(RefusalKind::Agent),
            "{path:?}"
        );
    }
}
