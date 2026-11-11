#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The caller gate of the task reads: markers, root classes and the order of port calls.
mod task_reads_support;

use hf_application::ctl::{
    state_inspect::RootClass,
    task_reads::{PageMode, ReadError, Resolution, StoreFailure, list},
};
use hf_domain::ctl::{
    caller::RefusalKind,
    inspect::{InspectReason, InspectStatus, StateInspection},
};
use task_reads_support::{
    EMPTY_MARKER, FULL_MARKER, NONE_MARKERS, World,
    gate::{ALL, Path, refused, request, run, world},
    page, root, task_id,
};

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
            "classify",
            "view",
            "snapshot",
            "damaged",
            "related",
            "lookup",
            "record_stop"
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
