#![cfg(target_os = "macos")]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! A change during the inventory walk is reported as `Changed`.
use hf_domain::ctl::recovery_proposal::{Inventory, InventoryFailure};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};
mod inventory_support;

use inventory_support::Fixture;

/// There is no seam between the reads of one entry, so a change during the walk is
/// provoked by a second thread that keeps creating and removing an entry in a directory
/// the walk lists: every outcome is a complete inventory or `Changed`, and the walk
/// reports `Changed` at least once.
fn mutate_while_walking(
    fixture: &Fixture,
    mutate: impl Fn(&PathBuf) + Send + 'static,
    directory: PathBuf,
) -> Vec<Result<Inventory, InventoryFailure>> {
    let stop = Arc::new(AtomicBool::new(false));
    let running = Arc::new(AtomicBool::new(false));
    let handle = {
        let (stop, running) = (Arc::clone(&stop), Arc::clone(&running));
        thread::spawn(move || {
            running.store(true, Ordering::SeqCst);
            while !stop.load(Ordering::SeqCst) {
                mutate(&directory);
            }
        })
    };
    while !running.load(Ordering::SeqCst) {
        thread::yield_now();
    }
    let mut outcomes = Vec::new();
    for _ in 0..300 {
        let outcome = fixture.walk(100_000);
        let changed = outcome == Err(InventoryFailure::Changed);
        outcomes.push(outcome);
        if changed {
            break;
        }
    }
    stop.store(true, Ordering::SeqCst);
    handle.join().unwrap();
    outcomes
}

fn big_tree() -> Fixture {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("hot")).unwrap();
    for number in 0..3000 {
        fs::write(fixture.0.join("hot").join(format!("f{number}")), b"").unwrap();
    }
    fixture
}

#[test]
fn change_during_walk_entry_created_and_removed_is_reported_as_changed() {
    let fixture = big_tree();
    let outcomes = mutate_while_walking(
        &fixture,
        |directory| {
            let flip = directory.join("flip");
            let _ = fs::write(&flip, b"x");
            let _ = fs::remove_file(&flip);
        },
        fixture.0.join("hot"),
    );
    assert!(
        outcomes
            .iter()
            .all(|o| matches!(o, Ok(_) | Err(InventoryFailure::Changed))),
        "{outcomes:?}"
    );
    assert!(
        outcomes.contains(&Err(InventoryFailure::Changed)),
        "no change was observed in {} walks",
        outcomes.len()
    );
}

#[test]
fn change_during_walk_lock_created_and_removed_is_reported_as_changed() {
    let fixture = big_tree();
    let outcomes = mutate_while_walking(
        &fixture,
        |directory| {
            let lock = directory.join(".harflow-store.lock");
            let _ = fs::write(&lock, b"");
            let _ = fs::remove_file(&lock);
        },
        fixture.0.clone(),
    );
    assert!(
        outcomes
            .iter()
            .all(|o| matches!(o, Ok(_) | Err(InventoryFailure::Changed))),
        "{outcomes:?}"
    );
    assert!(
        outcomes.contains(&Err(InventoryFailure::Changed)),
        "no change was observed in {} walks",
        outcomes.len()
    );
}

#[test]
fn change_during_walk_top_level_entry_created_and_removed_is_reported_as_changed() {
    let fixture = big_tree();
    let outcomes = mutate_while_walking(
        &fixture,
        |directory| {
            let flip = directory.join("flip");
            let _ = fs::write(&flip, b"x");
            let _ = fs::remove_file(&flip);
        },
        fixture.0.clone(),
    );
    assert!(
        outcomes
            .iter()
            .all(|o| matches!(o, Ok(_) | Err(InventoryFailure::Changed))),
        "{outcomes:?}"
    );
    assert!(
        outcomes.contains(&Err(InventoryFailure::Changed)),
        "no change was observed in {} walks",
        outcomes.len()
    );
}

/// Entries are only ever added at the top level, never removed, so a walk can see the
/// change only through the root's own attributes changing between its start and its end.
#[test]
fn change_during_walk_top_level_entries_only_created_is_reported_as_changed() {
    let fixture = big_tree();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let outcomes = mutate_while_walking(
        &fixture,
        move |directory| {
            let number = next.fetch_add(1, Ordering::SeqCst);
            let _ = fs::write(directory.join(format!("added-{number}")), b"");
            thread::sleep(std::time::Duration::from_micros(200));
        },
        fixture.0.clone(),
    );
    assert!(
        outcomes
            .iter()
            .all(|o| matches!(o, Ok(_) | Err(InventoryFailure::Changed))),
        "{outcomes:?}"
    );
    assert!(
        outcomes.contains(&Err(InventoryFailure::Changed)),
        "no change was observed in {} walks",
        outcomes.len()
    );
}
