use super::support::*;
use crate::fixture_support::{A, B, world};
use hf_application::task_runtime::{InvalidIntent, RuntimeParticipantDecoder};
use hf_domain::{
    ownership::{Phase, PreparedRequest},
    process::ProcessIdentity,
};
use hf_storage::Error;

struct TestParticipants;
impl RuntimeParticipantDecoder for TestParticipants {
    fn participants(
        &self,
        request: &PreparedRequest,
    ) -> Result<Vec<ProcessIdentity>, InvalidIntent> {
        let text = std::str::from_utf8(&request.payload).map_err(|_| InvalidIntent)?;
        let pid = text
            .strip_prefix("trusted-dispatcher:")
            .ok_or(InvalidIntent)?
            .parse::<u32>()
            .map_err(|_| InvalidIntent)?;
        Ok(vec![dispatcher(pid)])
    }
}

fn prepare(
    world: &crate::fixture_support::World,
    operation: u8,
    task: &str,
    payload: &str,
) -> (
    PreparedRequest,
    hf_domain::task_lifecycle::LifecycleEvidence,
) {
    let request = request_with_payload(&world.store, operation, payload.as_bytes().to_vec());
    let evidence = world
        .store
        .prepare_launch_intent(
            &request,
            &create(task, "Prepared inventory fixture."),
            &IntentGate {
                refuse: false,
                calls: std::cell::Cell::new(0),
            },
            later(5),
        )
        .unwrap();
    (request, evidence)
}

fn dispatcher(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(
        pid,
        1_768_000_100,
        u64::from(pid),
        "01234567-89ab-cdef-0123-456789abcdef".into(),
    )
    .unwrap()
}

#[test]
fn exact_prepared_exclusion_keeps_same_dispatcher_from_another_row_and_omits_shell_caller() {
    let world = world();
    let (current, _) = prepare(&world, 90, A, "trusted-dispatcher:6100");
    prepare(&world, 91, B, "trusted-dispatcher:6100");
    let shell_caller = dispatcher(6000);
    let candidates = world
        .store
        .runtime_mutation_candidates_except(&TestParticipants, 64, &current, later(5))
        .unwrap();
    assert_eq!(candidates, vec![dispatcher(6100)]);
    assert!(!candidates.contains(&shell_caller));
}

#[test]
fn prepared_inventory_is_dispatcher_only_then_claimed_inventory_is_owner_and_child() {
    let world = world();
    let (_, prepared) = prepare(&world, 92, A, "trusted-dispatcher:6200");
    let candidates = world
        .store
        .runtime_mutation_candidates(&TestParticipants, 64, later(5))
        .unwrap();
    assert_eq!(candidates, vec![dispatcher(6200)]);

    let child = child();
    let intent = world
        .store
        .read_launch_intent(&prepared.binding, later(5))
        .unwrap()
        .unwrap()
        .create;
    let task = task_for_spawn(A, &intent, &child);
    let (revision, fence) = claim(&world.store, &prepared.binding);
    world
        .store
        .publish_launch_spawn(
            &prepared.binding,
            &fence,
            revision,
            &spawn_publication(&task, &child, &SpawnGate::new(false)),
            later(5),
        )
        .unwrap();
    let candidates = world
        .store
        .runtime_mutation_candidates(&TestParticipants, 64, later(5))
        .unwrap();
    assert_eq!(candidates, vec![owner().process, child]);
    assert!(!candidates.contains(&dispatcher(6200)));
}

#[test]
fn exact_exclusion_refuses_changed_request_root_payload_and_nonprepared_phase() {
    let world = world();
    let (request, prepared) = prepare(&world, 93, A, "trusted-dispatcher:6300");
    let mut changed_payload = request.clone();
    changed_payload.payload = b"trusted-dispatcher:6301".to_vec();
    changed_payload.digest = digest(&changed_payload.payload);
    assert!(matches!(
        world.store.runtime_mutation_candidates_except(
            &TestParticipants,
            64,
            &changed_payload,
            later(5)
        ),
        Err(Error::StaleOwnership)
    ));
    let mut changed_root = request.clone();
    changed_root.generation = id(99);
    assert!(matches!(
        world.store.runtime_mutation_candidates_except(
            &TestParticipants,
            64,
            &changed_root,
            later(5)
        ),
        Err(Error::StaleOwnership)
    ));

    let (revision, fence) = claim(&world.store, &prepared.binding);
    assert!(matches!(
        world
            .store
            .runtime_mutation_candidates_except(&TestParticipants, 64, &request, later(5)),
        Err(Error::StaleOwnership)
    ));
    assert_eq!(fence.revision, revision);
    assert_eq!(prepared.ownership.phase, Phase::Prepared);
}

#[test]
fn malformed_legacy_and_overbudget_inventories_refuse_whole_result() {
    for (operation, payload) in [(94, "HFOPv1:legacy"), (95, "malformed")] {
        let world = world();
        prepare(&world, operation, A, payload);
        assert!(matches!(
            world
                .store
                .runtime_mutation_candidates(&TestParticipants, 64, later(5)),
            Err(Error::InvalidOwnership)
        ));
    }

    let world = world();
    prepare(&world, 96, A, "trusted-dispatcher:6400");
    prepare(&world, 97, B, "trusted-dispatcher:6401");
    assert!(matches!(
        world
            .store
            .runtime_mutation_candidates(&TestParticipants, 1, later(5)),
        Err(Error::LimitExceeded)
    ));
    assert!(matches!(
        world
            .store
            .runtime_mutation_candidates(&TestParticipants, 65, later(5)),
        Err(Error::LimitExceeded)
    ));
}
