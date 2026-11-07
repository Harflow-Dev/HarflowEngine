#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The artifact page read over scripted ports: entries, paging, cursors, manifests, the byte bound and ancestry.
mod task_artifacts_support;
mod task_reads_support;
use hf_application::ctl::task_reads::{
    ArtifactCursorArg, ArtifactEntry, ArtifactRequest, NextCursor, artifacts,
};
use hf_domain::ctl::history::ManifestHead;
use task_artifacts_support::*;
use task_reads_support::{NONE_MARKERS, World, record, root, task_id};
#[test]
fn the_page_reports_entries_in_order_with_the_observed_state_of_each_target() {
    let world = world(6);
    let found = targets();
    let page = artifacts(
        &world.reads(NONE_MARKERS),
        &found,
        &root(),
        &request(20, ArtifactCursorArg::Absent),
        &|_, e| e.len(),
    )
    .unwrap();
    let states: Vec<&str> = page
        .entries
        .iter()
        .map(|e: &ArtifactEntry| e.availability.as_str())
        .collect();
    assert_eq!(
        states,
        [
            "available",
            "changed",
            "deleted",
            "unavailable",
            "available",
            "available"
        ]
    );
    assert_eq!((page.count, page.has_more, page.readable), (6, false, true));
    assert!(found.seen.borrow().iter().all(|(repo, _)| repo == "/repo"));
    assert_eq!(found.seen.borrow().len(), 6);
}

#[test]
fn paging_continues_from_the_cursor_and_ends_without_one() {
    let world = world(7);
    let first = read(&world, NONE_MARKERS, &request(3, ArtifactCursorArg::Absent)).unwrap();
    assert_eq!((first.entries.len(), first.has_more), (3, true));
    let next = first.next.clone().unwrap();
    assert_eq!(
        (next.offset, next.revision.as_str(), next.task.as_str()),
        (3, REVISION, task_id(1).as_str())
    );
    let second = read(
        &world,
        NONE_MARKERS,
        &request(3, ArtifactCursorArg::Valid(next)),
    )
    .unwrap();
    assert_eq!(second.entries[0].item.artifact_id, "x3");
    let last = read(&world, NONE_MARKERS, &request(3, cursor(6))).unwrap();
    assert_eq!(
        (last.entries.len(), last.has_more, last.next),
        (1, false, None)
    );
}

#[test]
fn an_offset_equal_to_the_count_is_an_empty_final_page() {
    let page = read(&world(4), NONE_MARKERS, &request(3, cursor(4))).unwrap();
    assert_eq!(
        (page.entries.len(), page.has_more, page.next),
        (0, false, None)
    );
}

#[test]
fn a_cursor_of_another_task_revision_or_range_is_stale() {
    let world = world(4);
    let other_task = ArtifactCursorArg::Valid(NextCursor {
        task: task_id(2).as_str().to_string(),
        revision: REVISION.into(),
        offset: 1,
    });
    let other_revision = ArtifactCursorArg::Valid(NextCursor {
        task: task_id(1).as_str().to_string(),
        revision: "f".repeat(32),
        offset: 1,
    });
    for arg in [
        other_task,
        other_revision,
        cursor(5),
        cursor(-1),
        ArtifactCursorArg::Malformed,
    ] {
        assert_eq!(read(&world, NONE_MARKERS, &request(3, arg)), bad_cursor());
    }
}

#[test]
fn a_cursor_where_there_is_no_manifest_is_stale_and_none_is_an_empty_page() {
    let mut world = world(4);
    world.history.manifests.clear();
    let page = read(&world, NONE_MARKERS, &request(3, ArtifactCursorArg::Absent)).unwrap();
    assert_eq!(
        (page.readable, page.count, page.entries.len()),
        (true, 0, 0)
    );
    assert_eq!(
        read(&world, NONE_MARKERS, &request(3, cursor(0))),
        bad_cursor()
    );
    assert_eq!(world.calls("items"), 0);
}

#[test]
fn an_unreadable_manifest_has_no_entries_whatever_the_cursor() {
    for (valid, count) in [(false, 3), (true, 101)] {
        let mut world = world(3);
        world.history.manifests.insert(
            task_id(1).as_str().to_string(),
            ManifestHead {
                header_valid: valid,
                revision: REVISION.into(),
                count,
            },
        );
        for arg in [
            ArtifactCursorArg::Absent,
            cursor(0),
            ArtifactCursorArg::Malformed,
        ] {
            let page = read(&world, NONE_MARKERS, &request(3, arg)).unwrap();
            assert_eq!(
                (page.readable, page.count, page.entries.len(), page.has_more),
                (false, 0, 0, false)
            );
        }
        assert_eq!(world.calls("items"), 0);
    }
}

#[test]
fn one_hundred_items_are_readable_and_one_hundred_and_one_are_not() {
    for (count, readable) in [(100, true), (101, false)] {
        let mut world = world(count);
        world.history.manifests.insert(
            task_id(1).as_str().to_string(),
            ManifestHead {
                header_valid: true,
                revision: REVISION.into(),
                count,
            },
        );
        let page = read(
            &world,
            NONE_MARKERS,
            &request(50, ArtifactCursorArg::Absent),
        )
        .unwrap();
        assert_eq!(page.readable, readable, "{count}");
    }
}

#[test]
fn the_byte_bound_cuts_a_page_short_by_whole_entries_and_keeps_going_from_there() {
    let world = world(10);
    let page = artifacts(
        &world.reads(NONE_MARKERS),
        &targets(),
        &root(),
        &request(10, ArtifactCursorArg::Absent),
        &|_, e| e.len() * 20_000,
    )
    .unwrap();
    assert_eq!(
        page.entries.len(),
        3,
        "63,000 bytes hold three entries of 20,000"
    );
    assert_eq!(
        (page.has_more, page.next.as_ref().map(|n| n.offset)),
        (true, Some(3))
    );
    let none = artifacts(
        &world.reads(NONE_MARKERS),
        &targets(),
        &root(),
        &request(10, ArtifactCursorArg::Absent),
        &|_, _| 70_000,
    )
    .unwrap();
    assert!(
        none.entries.is_empty(),
        "an entry above the bound is left for no one"
    );
}

#[test]
fn the_bound_is_exactly_sixty_three_thousand_bytes() {
    let world = world(3);
    let at = artifacts(
        &world.reads(NONE_MARKERS),
        &targets(),
        &root(),
        &request(3, ArtifactCursorArg::Absent),
        &|_, e| {
            if e.len() == 3 { 63_000 } else { 1 }
        },
    )
    .unwrap();
    assert_eq!(at.entries.len(), 3);
    let over = artifacts(
        &world.reads(NONE_MARKERS),
        &targets(),
        &root(),
        &request(3, ArtifactCursorArg::Absent),
        &|_, e| {
            if e.len() == 3 { 63_001 } else { 1 }
        },
    )
    .unwrap();
    assert_eq!(over.entries.len(), 2);
}

fn chain(world: &mut World, length: u32, tip_link: Option<u32>) {
    world.tasks.clear();
    for number in 1..=length {
        let mut task = record(number, "completed", Some(0), None);
        task.facts.continuation_of = if number > 1 {
            Some(task_id(number - 1))
        } else {
            tip_link.map(task_id)
        };
        world.tasks.push(task);
    }
}

fn tip(length: u32) -> ArtifactRequest {
    let mut asked = request(20, ArtifactCursorArg::Absent);
    asked.task_id = task_id(length).as_str().to_string();
    asked
}

#[test]
fn a_missing_or_cyclic_ancestor_refuses_before_the_manifest_is_read() {
    let mut world = World::new();
    world.history.noisy = true;
    chain(&mut world, 2, Some(900));
    assert_eq!(
        read(&world, NONE_MARKERS, &tip(2)),
        refused("Task ancestry is unavailable")
    );
    chain(&mut world, 3, Some(3));
    assert_eq!(
        read(&world, NONE_MARKERS, &tip(3)),
        refused("Task ancestry is cyclic")
    );
    chain(&mut world, 1, Some(1));
    assert_eq!(
        read(&world, NONE_MARKERS, &tip(1)),
        refused("Task ancestry is cyclic")
    );
    assert_eq!(world.calls("manifest"), 0);
}

#[test]
fn a_chain_of_one_hundred_tasks_is_readable_and_one_hundred_and_one_are_not() {
    let mut world = World::new();
    world.history.noisy = true;
    chain(&mut world, 100, None);
    assert!(read(&world, NONE_MARKERS, &tip(100)).is_ok());
    chain(&mut world, 101, None);
    assert_eq!(
        read(&world, NONE_MARKERS, &tip(101)),
        refused("Task ancestry exceeds authority bounds")
    );
}

#[test]
fn ancestry_is_walked_through_continuation_links_only_and_never_reads_a_record() {
    let mut world = World::new();
    world.history.noisy = true;
    chain(&mut world, 5, None);
    read(&world, NONE_MARKERS, &tip(5)).unwrap();
    assert_eq!(world.calls("link"), 4);
    assert_eq!(
        world.calls("record"),
        1,
        "only the task itself is read as a record"
    );
}
