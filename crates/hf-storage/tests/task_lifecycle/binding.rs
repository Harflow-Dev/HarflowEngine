use super::support::*;
use crate::{
    fixture_support::{A, later, world},
    task_writer_support::running,
};
use hf_storage::Error;

#[test]
fn lifecycle_binding_rejects_wrong_attempt_and_wrong_generation() {
    let world = world();
    let task = running(A);
    let request = request(&world.store, 8);
    let evidence = world
        .store
        .prepare_lifecycle(&request, &task, &ACCEPT, later(5))
        .unwrap();
    let mut wrong_attempt = evidence.binding.clone();
    wrong_attempt.attempt = id(99);
    assert!(matches!(
        world.store.read_lifecycle(&wrong_attempt, later(5)),
        Err(Error::StaleOwnership)
    ));
    let mut wrong_generation = evidence.binding.clone();
    wrong_generation.generation = id(98);
    assert!(matches!(
        world.store.read_lifecycle(&wrong_generation, later(5)),
        Err(Error::StaleOwnership)
    ));
}
