use super::support::{IntentGate, create, request, spawn_task};
use crate::fixture_support::{self, A, world};
use hf_application::task_runtime::{InvalidIntent, PreAgentRefusalAdmission};
use hf_domain::{ownership::Phase, task_lifecycle::LifecycleEvidence};
use hf_storage::Error;
use std::cell::Cell;

struct RefusalGate {
    calls: Cell<usize>,
    refuse: bool,
}

impl PreAgentRefusalAdmission for RefusalGate {
    fn admit(&self, evidence: &LifecycleEvidence, reason: &str) -> Result<(), InvalidIntent> {
        self.calls.set(self.calls.get() + 1);
        if self.refuse
            || evidence.ownership.phase != Phase::Prepared
            || evidence.ownership.child.is_some()
            || reason != "unsupported_version"
        {
            Err(InvalidIntent)
        } else {
            Ok(())
        }
    }
}

fn gate(refuse: bool) -> RefusalGate {
    RefusalGate {
        calls: Cell::new(0),
        refuse,
    }
}

#[test]
fn refusal_admission_rolls_back_and_positive_not_started_stays_private() {
    let world = world();
    let request = request(&world.store, 111);
    let prepared = world
        .store
        .prepare_launch_intent(
            &request,
            &create(A, "Review the repository."),
            &IntentGate {
                refuse: false,
                calls: Cell::new(0),
            },
            fixture_support::later(5),
        )
        .unwrap();
    let before_watch = world.watch();
    let rejected = gate(true);
    assert!(matches!(
        world.store.record_launch_not_started(
            &prepared.binding,
            None,
            0,
            "unsupported_version",
            &rejected,
            fixture_support::later(5)
        ),
        Err(Error::InvalidOwnership)
    ));
    assert_eq!(rejected.calls.get(), 1);
    assert_rolled_back(&world, &prepared.binding, before_watch);

    let admitted = gate(false);
    let evidence = world
        .store
        .record_launch_not_started(
            &prepared.binding,
            None,
            0,
            "unsupported_version",
            &admitted,
            fixture_support::later(5),
        )
        .unwrap();
    assert_eq!(admitted.calls.get(), 1);
    assert_eq!(evidence.revision, 1);
    assert_eq!(evidence.ownership.phase, Phase::Prepared);
    assert_not_started_is_private(&world, before_watch);
}

fn assert_rolled_back(
    world: &fixture_support::World,
    binding: &hf_domain::task_lifecycle::LifecycleBinding,
    before_watch: i64,
) {
    assert_eq!(world.watch(), before_watch);
    assert_eq!(
        fixture_support::count(&world.db(), "task_facts", "id", A),
        0
    );
    assert_eq!(
        fixture_support::one::<String>(
            &world.db(),
            "SELECT state FROM launch_attempts WHERE name=?1",
            [A]
        ),
        "reserved"
    );
    assert_eq!(
        fixture_support::one::<Option<String>>(
            &world.db(),
            "SELECT not_started_reason FROM launch_attempts WHERE name=?1",
            [A]
        ),
        None
    );
    assert_eq!(
        world
            .store
            .read_lifecycle(binding, fixture_support::later(5))
            .unwrap()
            .unwrap()
            .revision,
        0
    );
}

fn assert_not_started_is_private(world: &fixture_support::World, before_watch: i64) {
    assert_eq!(
        fixture_support::count(&world.db(), "task_facts", "id", A),
        0
    );
    assert_eq!(
        fixture_support::count(&world.db(), "task_headers", "id", A),
        0
    );
    assert_eq!(
        fixture_support::count(&world.db(), "task_processes", "task", A),
        0
    );
    assert_eq!(
        fixture_support::count(&world.db(), "event_records", "task", A),
        0
    );
    assert_eq!(
        fixture_support::count(&world.db(), "event_streams", "task", A),
        0
    );
    assert_eq!(world.watch(), before_watch);
    assert_eq!(
        fixture_support::one::<String>(
            &world.db(),
            "SELECT state FROM launch_attempts WHERE name=?1",
            [A]
        ),
        "not_started"
    );
    assert_eq!(
        fixture_support::one::<String>(
            &world.db(),
            "SELECT not_started_reason FROM launch_attempts WHERE name=?1",
            [A]
        ),
        "unsupported_version"
    );
}

#[test]
fn child_publication_cannot_be_reclassified_as_no_agent_started() {
    let world = world();
    let spawned = spawn_task(&world, A, 112, "Review the repository.");
    let before_watch = world.watch();
    let admission = gate(false);
    assert!(matches!(
        world.store.record_launch_not_started(
            &spawned.evidence.binding,
            Some(&spawned.fence),
            spawned.evidence.revision,
            "unsupported_version",
            &admission,
            fixture_support::later(5)
        ),
        Err(Error::StaleOwnership)
    ));
    assert_eq!(admission.calls.get(), 0);
    assert_eq!(world.watch(), before_watch);
    assert_eq!(
        fixture_support::count(&world.db(), "task_facts", "id", A),
        1
    );
    assert_eq!(
        fixture_support::one::<String>(
            &world.db(),
            "SELECT state FROM launch_attempts WHERE name=?1",
            [A]
        ),
        "spawned"
    );
    let retained = world
        .store
        .read_lifecycle(&spawned.evidence.binding, fixture_support::later(5))
        .unwrap()
        .unwrap();
    assert_eq!(retained.ownership.child.as_ref(), Some(&spawned.child));
}
