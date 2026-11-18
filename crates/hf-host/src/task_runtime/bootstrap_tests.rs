#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::{HeldRoot, Rc};
use crate::task_runtime::store_options;
use hf_storage::{CheckpointPolicy, Limits, Maintenance, RootHold, Store};
use std::{
    fs,
    path::PathBuf,
    sync::MutexGuard,
    time::{Duration, Instant},
};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

/// A real root and a retained hold on it, as the task writer has.
fn held(name: &str) -> (MutexGuard<'static, ()>, Scratch, HeldRoot) {
    // The platform lease state is process-wide: every in-process root shares one lock.
    let guard = crate::task_runtime::tests::serial_case();
    let parent = std::env::temp_dir().join(format!("hf-host-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let scratch = Scratch(fs::canonicalize(parent).unwrap());
    let root = scratch.0.join("store");
    let leases = hf_platform::lease::shared;
    drop(
        Store::initialize_runtime(
            &root,
            Limits::default(),
            deadline(),
            leases(),
            store_options::suppressed(),
        )
        .unwrap(),
    );
    let hold = RootHold::open(&root, deadline(), leases()).unwrap();
    let held = HeldRoot {
        root,
        hold: Rc::new(hold),
    };
    (guard, scratch, held)
}

#[test]
fn writer_runtime_opens_store_with_owner_policy() {
    let (_guard, _scratch, held) = held("writer-policy");
    let store = held.open_writer_store(deadline()).unwrap();
    let options = store.writer_options();
    assert_eq!(options.checkpoint, CheckpointPolicy::Owner);
    assert_eq!(options.maintenance, Maintenance::Idle);
}

#[test]
fn dispatcher_and_readers_keep_suppressed_policy() {
    let (_guard, _scratch, held) = held("suppressed-policy");
    let validation = held.open_validation_store(deadline()).unwrap();
    let options = validation.writer_options();
    assert_eq!(options.checkpoint, CheckpointPolicy::Suppressed);
    assert_eq!(options.maintenance, Maintenance::AfterCommit);
    let dispatcher = store_options::suppressed();
    assert_eq!(dispatcher.checkpoint, CheckpointPolicy::Suppressed);
    assert_eq!(dispatcher.maintenance, Maintenance::AfterCommit);
}
