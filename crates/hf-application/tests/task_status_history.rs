#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The artifact summary and the recent event bodies that `status` and `status-batch` carry.
mod task_reads_support;

use hf_application::ctl::task_reads::{status, status_batch, status_with};
use hf_domain::ctl::history::{ArtifactSummary, ManifestHead};
use task_reads_support::{NONE_MARKERS, World, record, root, task_id};

fn world() -> World {
    let mut world = World::new();
    world.history.noisy = true;
    world.tasks.push(record(1, "completed", Some(0), None));
    world
}

fn summary_of(world: &World) -> ArtifactSummary {
    status(&world.reads(NONE_MARKERS), &root(), task_id(1).as_str())
        .unwrap()
        .artifacts
}

fn manifest(world: &mut World, header_valid: bool, count: usize) {
    world.history.manifests.insert(
        task_id(1).as_str().to_string(),
        ManifestHead {
            header_valid,
            revision: "r".repeat(32),
            count,
        },
    );
}

#[test]
fn a_task_without_a_manifest_has_an_available_empty_summary_with_a_reference() {
    assert_eq!(
        summary_of(&world()),
        ArtifactSummary {
            count: 0,
            available: true,
            detail_ref: true
        }
    );
}

#[test]
fn a_readable_manifest_counts_its_items() {
    for count in [0, 1, 100] {
        let mut world = world();
        manifest(&mut world, true, count);
        assert_eq!(
            summary_of(&world),
            ArtifactSummary {
                count,
                available: true,
                detail_ref: true
            }
        );
    }
}

#[test]
fn an_unreadable_manifest_is_unavailable_with_no_count_but_keeps_its_reference() {
    for (valid, count) in [(false, 3), (true, 101)] {
        let mut world = world();
        manifest(&mut world, valid, count);
        assert_eq!(
            summary_of(&world),
            ArtifactSummary {
                count: 0,
                available: false,
                detail_ref: true
            }
        );
    }
}

#[test]
fn broken_ancestry_is_unavailable_with_no_reference_and_never_reads_the_manifest() {
    let mut world = world();
    world.tasks[0].facts.continuation_of = Some(task_id(900));
    manifest(&mut world, true, 3);
    assert_eq!(
        summary_of(&world),
        ArtifactSummary {
            count: 0,
            available: false,
            detail_ref: false
        }
    );
    assert_eq!(world.calls("manifest"), 0);
}

#[test]
fn only_a_brief_status_reads_the_recent_bodies_and_in_the_same_snapshot() {
    let mut world = world();
    world.history.recent = vec![b"{}".to_vec()];
    let plain = status_with(
        &world.reads(NONE_MARKERS),
        &root(),
        task_id(1).as_str(),
        false,
    )
    .unwrap();
    assert_eq!(plain.recent_bodies, None);
    assert_eq!(world.calls("recent"), 0);
    let brief = status_with(
        &world.reads(NONE_MARKERS),
        &root(),
        task_id(1).as_str(),
        true,
    )
    .unwrap();
    assert_eq!(brief.recent_bodies, Some(vec![b"{}".to_vec()]));
    assert_eq!(world.calls("recent"), 1);
    assert_eq!(world.calls("snapshot"), 2, "one snapshot per command");
}

#[test]
fn the_batch_carries_a_summary_per_row() {
    let mut world = world();
    world.tasks.push(record(2, "completed", Some(0), None));
    world.history.manifests.insert(
        task_id(2).as_str().to_string(),
        ManifestHead {
            header_valid: true,
            revision: "r".repeat(32),
            count: 4,
        },
    );
    let ids = [
        task_id(1).as_str().to_string(),
        task_id(2).as_str().to_string(),
    ];
    let out = status_batch(&world.reads(NONE_MARKERS), &root(), &ids).unwrap();
    let counts: Vec<usize> = out.rows.iter().map(|row| row.artifacts.count).collect();
    assert_eq!(counts, [0, 4]);
}
