use super::support::*;
use crate::fixture_support::{A, world};
use hf_domain::ownership::Phase;
use hf_storage::Error;

fn claim_and_spawn(world: &crate::fixture_support::World, operation: u8) -> SpawnedTask {
    spawn_task(world, A, operation, "Run after acceptance.")
}

#[test]
fn acceptance_requires_a_recorded_child() {
    let world = world();
    let request = request(&world.store, 80);
    let intent = create(A, "Run after acceptance.");
    let prepared = world
        .store
        .prepare_launch_intent(
            &request,
            &intent,
            &IntentGate {
                refuse: false,
                calls: std::cell::Cell::new(0),
            },
            later(5),
        )
        .unwrap();
    let (revision, fence) = claim(&world.store, &prepared.binding);
    let gate = AcceptanceGate {
        refuse: false,
        calls: std::cell::Cell::new(0),
    };
    let before = world.watch();
    assert!(matches!(
        world.store.record_launch_acceptance(
            &prepared.binding,
            &fence,
            revision,
            &readiness(&child()),
            &gate,
            later(5)
        ),
        Err(Error::StaleOwnership)
    ));
    assert_eq!(gate.calls.get(), 0);
    assert_eq!(world.watch(), before);
}

#[test]
fn acceptance_refusal_rolls_back_and_success_records_no_release() {
    let world = world();
    let spawned = claim_and_spawn(&world, 81);
    let before = world.watch();
    let rejected = AcceptanceGate {
        refuse: true,
        calls: std::cell::Cell::new(0),
    };
    assert!(matches!(
        world.store.record_launch_acceptance(
            &spawned.evidence.binding,
            &spawned.fence,
            spawned.evidence.revision,
            &readiness(&spawned.child),
            &rejected,
            later(5)
        ),
        Err(Error::InvalidOwnership)
    ));
    assert_eq!(rejected.calls.get(), 1);
    assert_eq!(world.watch(), before);
    assert_eq!(
        world
            .store
            .read_lifecycle(&spawned.evidence.binding, later(5))
            .unwrap()
            .unwrap()
            .ownership
            .phase,
        Phase::SpawnObserved
    );

    let gate = AcceptanceGate {
        refuse: false,
        calls: std::cell::Cell::new(0),
    };
    let accepted = world
        .store
        .record_launch_acceptance(
            &spawned.evidence.binding,
            &spawned.fence,
            spawned.evidence.revision,
            &readiness(&spawned.child),
            &gate,
            later(5),
        )
        .unwrap();
    assert_eq!(accepted.ownership.phase, Phase::AcceptanceRecorded);
    assert_eq!(accepted.revision, spawned.evidence.revision + 1);
    assert_eq!(world.watch(), before);
    assert_eq!(
        crate::fixture_support::one::<i64>(
            &world.db(),
            "SELECT count(*) FROM lifecycle_acceptance",
            []
        ),
        1
    );
    assert_eq!(release_proof(&world, &spawned.evidence.binding), None);
}
