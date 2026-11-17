use super::support::*;
use crate::fixture_support::{A, B, one, world};
use hf_storage::Error;

fn public_rows(world: &crate::fixture_support::World) -> Vec<i64> {
    let db = world.db();
    [
        ("task_facts", "id"),
        ("task_headers", "id"),
        ("task_prompts", "task"),
        ("task_processes", "task"),
        ("event_streams", "task"),
        ("event_records", "task"),
    ]
    .into_iter()
    .map(|(table, _key)| one(&db, &format!("SELECT count(*) FROM {table}"), []))
    .collect()
}

#[test]
fn private_t0_persists_intent_and_reservation_without_a_public_task_or_watch_bump() {
    let world = world();
    let request = request(&world.store, 60);
    let intent = create(A, "Summarize the repository.");
    let validator = IntentGate {
        refuse: false,
        calls: std::cell::Cell::new(0),
    };
    let watch_before = world.watch();

    let evidence = world
        .store
        .prepare_launch_intent(&request, &intent, &validator, later(5))
        .unwrap();
    assert_eq!(validator.calls.get(), 1);
    assert_eq!(evidence.binding.task.as_str(), A);
    assert_eq!(evidence.binding.attempt, request.attempt);
    assert_eq!(evidence.revision, 0);
    assert_eq!(evidence.ownership.child, None);
    assert_eq!(world.watch(), watch_before);
    assert_eq!(public_rows(&world), vec![0; 6]);

    let db = world.db();
    assert_eq!(
        one::<i64>(&db, "SELECT count(*) FROM lifecycle_intents", []),
        1
    );
    assert_eq!(
        one::<i64>(&db, "SELECT count(*) FROM lifecycle_records", []),
        1
    );
    assert_eq!(
        one::<i64>(&db, "SELECT count(*) FROM ownership_records", []),
        1
    );
    assert_eq!(
        one::<String>(&db, "SELECT state FROM launch_attempts WHERE name=?1", [A]),
        "reserved"
    );
    let stored = world
        .store
        .read_launch_intent(&evidence.binding, later(5))
        .unwrap()
        .unwrap();
    assert_eq!(stored.create, intent);
    assert_eq!(stored.preparation_digest, request.digest);
    let mut wrong_attempt = evidence.binding.clone();
    wrong_attempt.attempt = id(99);
    assert!(matches!(
        world.store.read_launch_intent(&wrong_attempt, later(5)),
        Err(Error::StaleOwnership)
    ));
    let mut wrong_generation = evidence.binding.clone();
    wrong_generation.generation = id(98);
    assert!(matches!(
        world.store.read_launch_intent(&wrong_generation, later(5)),
        Err(Error::StaleOwnership)
    ));
}

#[test]
fn private_t0_rejects_oversize_intent_and_bad_digests_before_rows_or_validator_calls() {
    let world = world();
    let validator = IntentGate {
        refuse: false,
        calls: std::cell::Cell::new(0),
    };
    let watch_before = world.watch();
    let mut metadata_too_large = create(A, "prompt");
    metadata_too_large.metadata = vec![b'm'; 16_385];
    metadata_too_large.metadata_digest = digest(&metadata_too_large.metadata);
    let error = world
        .store
        .prepare_launch_intent(
            &request(&world.store, 61),
            &metadata_too_large,
            &validator,
            later(5),
        )
        .unwrap_err();
    assert!(matches!(error, Error::LimitExceeded));

    let mut prompt_too_large = create(B, "prompt");
    prompt_too_large.prompt = "p".repeat(4 * 1024 * 1024 + 1);
    prompt_too_large.prompt_digest = digest(prompt_too_large.prompt.as_bytes());
    let error = world
        .store
        .prepare_launch_intent(
            &request(&world.store, 62),
            &prompt_too_large,
            &validator,
            later(5),
        )
        .unwrap_err();
    assert!(matches!(error, Error::LimitExceeded));

    let mut bad_metadata_digest = create(A, "prompt");
    bad_metadata_digest.metadata_digest = digest(b"different metadata");
    let error = world
        .store
        .prepare_launch_intent(
            &request(&world.store, 63),
            &bad_metadata_digest,
            &validator,
            later(5),
        )
        .unwrap_err();
    assert!(matches!(error, Error::InvalidOwnership));

    let mut bad_prompt_digest = create(B, "prompt");
    bad_prompt_digest.prompt_digest = digest(b"different prompt");
    let error = world
        .store
        .prepare_launch_intent(
            &request(&world.store, 64),
            &bad_prompt_digest,
            &validator,
            later(5),
        )
        .unwrap_err();
    assert!(matches!(error, Error::InvalidOwnership));

    assert_eq!(validator.calls.get(), 0);
    assert_eq!(world.watch(), watch_before);
    assert_eq!(public_rows(&world), vec![0; 6]);
    let db = world.db();
    for table in [
        "lifecycle_intents",
        "lifecycle_records",
        "ownership_records",
    ] {
        assert_eq!(
            one::<i64>(&db, &format!("SELECT count(*) FROM {table}"), []),
            0
        );
    }
}

#[test]
fn private_t0_accepts_exact_metadata_and_prompt_caps_under_default_store_limits() {
    let world = world();
    let request = request(&world.store, 66);
    let mut intent = create(A, "");
    intent.metadata = vec![b'm'; 16_384];
    intent.metadata_digest = digest(&intent.metadata);
    intent.prompt = "p".repeat(4 * 1024 * 1024);
    intent.prompt_digest = digest(intent.prompt.as_bytes());
    let validator = IntentGate {
        refuse: false,
        calls: std::cell::Cell::new(0),
    };
    let watch_before = world.watch();

    let evidence = world
        .store
        .prepare_launch_intent(&request, &intent, &validator, later(5))
        .unwrap();
    assert_eq!(validator.calls.get(), 1);
    assert_eq!(
        world
            .store
            .read_launch_intent(&evidence.binding, later(5))
            .unwrap()
            .unwrap()
            .create,
        intent
    );
    assert_eq!(world.watch(), watch_before);
    assert_eq!(public_rows(&world), vec![0; 6]);
}

#[test]
fn rejected_initial_intent_admission_commits_no_private_or_public_state() {
    let world = world();
    let request = request(&world.store, 67);
    let intent = create(A, "Summarize the repository.");
    let validator = IntentGate {
        refuse: true,
        calls: std::cell::Cell::new(0),
    };
    let watch_before = world.watch();

    let error = world
        .store
        .prepare_launch_intent(&request, &intent, &validator, later(5))
        .unwrap_err();
    assert!(matches!(error, Error::InvalidOwnership));
    assert_eq!(validator.calls.get(), 1);
    assert_eq!(world.watch(), watch_before);
    assert_eq!(public_rows(&world), vec![0; 6]);
    let db = world.db();
    for table in [
        "lifecycle_intents",
        "lifecycle_records",
        "ownership_records",
    ] {
        assert_eq!(
            one::<i64>(&db, &format!("SELECT count(*) FROM {table}"), []),
            0
        );
    }
    assert_eq!(
        one::<i64>(&db, "SELECT count(*) FROM launch_attempts", []),
        0
    );
}

#[test]
fn a_late_private_t0_failure_rolls_back_every_intent_and_reservation_row() {
    let world = world();
    world
        .raw()
        .execute_batch(
            "CREATE TRIGGER reject_intent BEFORE INSERT ON lifecycle_intents
             BEGIN SELECT RAISE(ABORT, 'injected intent failure'); END;",
        )
        .unwrap();
    let request = request(&world.store, 65);
    let intent = create(A, "Summarize the repository.");
    let validator = IntentGate {
        refuse: false,
        calls: std::cell::Cell::new(0),
    };
    let before_sequence = one::<i64>(
        &world.db(),
        "SELECT value FROM commit_sequence WHERE singleton=1",
        [],
    );
    let before_watch = world.watch();

    let error = world
        .store
        .prepare_launch_intent(&request, &intent, &validator, later(5))
        .unwrap_err();
    assert!(matches!(error, Error::Sqlite(_)));
    let db = world.db();
    for table in [
        "ownership_records",
        "lifecycle_records",
        "lifecycle_intents",
    ] {
        assert_eq!(
            one::<i64>(&db, &format!("SELECT count(*) FROM {table}"), []),
            0
        );
    }
    assert_eq!(
        one::<i64>(&db, "SELECT count(*) FROM launch_attempts", []),
        0
    );
    assert_eq!(public_rows(&world), vec![0; 6]);
    assert_eq!(world.watch(), before_watch);
    assert_eq!(
        one::<i64>(
            &db,
            "SELECT value FROM commit_sequence WHERE singleton=1",
            []
        ),
        before_sequence
    );
}
