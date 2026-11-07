#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The caller gate of the task reads over scripted ports: verdicts, families, markers and
//! the order of port calls.
mod task_reads_support;

use hf_application::ctl::{
    state_inspect::RootClass,
    task_reads::{
        CursorArg, PageMode, PageRequest, ReadError, Resolution, StoreFailure, ViewFailure, list,
        status, status_batch,
    },
};
use hf_domain::{
    ctl::{
        caller::{EnvMarkers, PidObservation, ProcessView, RefusalKind},
        inspect::{InspectReason, InspectStatus, StateInspection},
    },
    process::ProcessIdentity,
};
use task_reads_support::{
    BOOT, EMPTY_MARKER, FULL_MARKER, NONE_MARKERS, World, caller, identity, page, record, root,
    task_id,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Path {
    Status,
    Batch,
    List,
    Plain,
    Ids,
    Successors,
}

const ALL: [Path; 6] = [
    Path::Status,
    Path::Batch,
    Path::List,
    Path::Plain,
    Path::Ids,
    Path::Successors,
];

fn request(mode: PageMode) -> PageRequest {
    PageRequest {
        mode,
        limit: 10,
        cursor: CursorArg::Absent,
        active_only: false,
        session: None,
    }
}

fn run(world: &World, markers: EnvMarkers, path: Path) -> Result<(), ReadError> {
    let reads = world.reads(markers);
    let first = task_id(1);
    match path {
        Path::Status => status(&reads, &root(), first.as_str()).map(|_| ()),
        Path::Batch => status_batch(&reads, &root(), &[first.as_str().to_string()]).map(|_| ()),
        Path::List => list(&reads, &root(), None).map(|_| ()),
        Path::Plain => page(&reads, &root(), &request(PageMode::Plain)).map(|_| ()),
        Path::Ids => page(
            &reads,
            &root(),
            &request(PageMode::TaskIds(vec![first.as_str().to_string()])),
        )
        .map(|_| ()),
        Path::Successors => page(
            &reads,
            &root(),
            &request(PageMode::Successors(first.as_str().to_string())),
        )
        .map(|_| ()),
    }
}

fn world() -> World {
    let mut world = World::new();
    world.tasks.push(record(1, "completed", Some(0), None));
    world
}

fn refused(kind: RefusalKind) -> Result<(), ReadError> {
    Err(ReadError::Refused(kind))
}

fn stale(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 1, 1, BOOT.to_string()).unwrap()
}

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

#[test]
fn an_effective_marker_refuses_before_the_process_view_and_per_path_order() {
    for path in [Path::Status, Path::Batch, Path::List] {
        let world = world();
        assert_eq!(run(&world, FULL_MARKER, path), refused(RefusalKind::Agent));
        assert_eq!(world.order(), ["classify"], "{path:?}");
    }
    for path in [Path::Plain, Path::Ids] {
        let world = world();
        assert_eq!(run(&world, FULL_MARKER, path), refused(RefusalKind::Agent));
        assert_eq!(
            world.order(),
            ["classify", "snapshot", "damaged"],
            "{path:?}"
        );
    }
    let world = world();
    assert_eq!(
        run(&world, FULL_MARKER, Path::Successors),
        refused(RefusalKind::Agent)
    );
    assert!(
        world.order().is_empty(),
        "the successor marker comes before the root"
    );
}

#[test]
fn without_a_marker_the_order_is_root_view_snapshot_damage_detection() {
    let world = world();
    assert_eq!(run(&world, NONE_MARKERS, Path::Status), Ok(()));
    assert_eq!(
        world.order(),
        [
            "classify", "view", "snapshot", "damaged", "related", "lookup", "record"
        ]
    );
    let world = self::world();
    run(&world, EMPTY_MARKER, Path::List).unwrap();
    assert_eq!(
        &world.order()[..4],
        ["classify", "view", "snapshot", "damaged"]
    );
}

#[test]
fn only_the_successor_gate_counts_empty_markers() {
    for path in ALL {
        let wanted = if path == Path::Successors {
            refused(RefusalKind::Agent)
        } else {
            Ok(())
        };
        assert_eq!(run(&world(), EMPTY_MARKER, path), wanted, "{path:?}");
    }
}

#[test]
fn a_refused_root_outranks_markers_except_for_successors() {
    let inspection = StateInspection::new(
        &root(),
        InspectStatus::Incompatible,
        InspectReason::ForeignStore,
        Vec::new(),
    );
    for path in ALL {
        let mut world = world();
        world.class = RootClass::Refused(inspection.clone());
        let wanted = if path == Path::Successors {
            refused(RefusalKind::Agent)
        } else {
            Err(ReadError::State(inspection.clone()))
        };
        assert_eq!(run(&world, FULL_MARKER, path), wanted, "{path:?}");
        assert_eq!(
            run(&world, NONE_MARKERS, path),
            Err(ReadError::State(inspection.clone())),
            "{path:?}"
        );
    }
}

#[test]
fn damage_outranks_a_marker_on_the_page_and_is_undecided_elsewhere() {
    let mut world = world();
    world.damaged = true;
    let reads = world.reads(FULL_MARKER);
    let out = page(&reads, &root(), &request(PageMode::Plain)).unwrap();
    assert!(out.withheld.is_some() && out.items.is_empty() && out.counts.is_none());
    assert_eq!(world.calls("view"), 0);
    assert_eq!(world.calls("related"), 0);
    let ids = page(
        &reads,
        &root(),
        &request(PageMode::TaskIds(vec![task_id(1).as_str().to_string()])),
    )
    .unwrap();
    assert!(ids.withheld.is_some());
    for path in [Path::Status, Path::Batch, Path::List, Path::Successors] {
        assert_eq!(
            run(&world, FULL_MARKER, path),
            refused(RefusalKind::Agent),
            "{path:?}"
        );
    }
    for path in [Path::Status, Path::List, Path::Successors] {
        assert_eq!(
            run(&world, NONE_MARKERS, path),
            Err(ReadError::Damaged),
            "{path:?}"
        );
    }
}

#[test]
fn roots_without_a_store_answer_without_opening_anything() {
    for class in [RootClass::Absent, RootClass::Empty] {
        for path in ALL {
            let mut world = world();
            world.class = class.clone();
            let outcome = run(&world, NONE_MARKERS, path);
            match path {
                Path::Status | Path::Successors => {
                    assert!(matches!(
                        outcome,
                        Err(ReadError::Resolution(Resolution::Unknown {
                            reason: None,
                            ..
                        }))
                    ));
                }
                _ => assert_eq!(outcome, Ok(()), "{path:?}"),
            }
            assert_eq!(world.order(), ["classify"], "{path:?}");
            let marked = run(&world, FULL_MARKER, path);
            assert_eq!(marked, refused(RefusalKind::Agent), "{path:?}");
            let empty = run(&world, EMPTY_MARKER, path);
            assert_eq!(
                empty.is_err(),
                matches!(path, Path::Status | Path::Successors),
                "{path:?}"
            );
        }
    }
}

#[test]
fn a_store_that_cannot_open_and_an_expired_deadline_are_busy() {
    let mut world = world();
    world.open_error = Some(StoreFailure::Busy);
    assert_eq!(
        run(&world, NONE_MARKERS, Path::List),
        Err(ReadError::Store(StoreFailure::Busy))
    );
    let world = World::new();
    let mut reads = world.reads(NONE_MARKERS);
    reads.deadline = std::time::Instant::now();
    assert_eq!(
        list(&reads, &root(), None).map(|_| ()),
        Err(ReadError::Store(StoreFailure::Busy)),
        "an overrun is busy, never a partial list"
    );
}
