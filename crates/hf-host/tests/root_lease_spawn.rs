#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_domain::process::ProcessExit;
use hf_platform::process::{
    MacIdentity, OwnedSignal, RetainedOwnedChild, SignalOutcome, SpawnInitialization, SpawnSpec,
    WaitOutcome,
};
use hf_storage::{Error, Limits, Store};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(3)
}

struct ChildGuard(RetainedOwnedChild);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.0.close_stdin();
        match self
            .0
            .wait_until(Instant::now() + Duration::from_millis(500), || false)
        {
            Ok(WaitOutcome::Exited(_)) => return,
            Ok(WaitOutcome::DeadlineExpired) => (),
            _ => std::process::abort(),
        }
        let Some(identity) = self.0.registered_identity().cloned() else {
            std::process::abort();
        };
        match self.0.request_signal(
            &identity,
            &MacIdentity,
            OwnedSignal::Kill,
            Instant::now() + Duration::from_millis(500),
        ) {
            Ok(
                SignalOutcome::Sent
                | SignalOutcome::Refused(hf_domain::process::IdentityObservation::Dead),
            ) => (),
            _ => std::process::abort(),
        }
        if !matches!(
            self.0
                .wait_until(Instant::now() + Duration::from_secs(2), || false),
            Ok(WaitOutcome::Exited(_))
        ) {
            std::process::abort();
        }
    }
}

#[test]
fn live_storage_root_lease_remains_held_through_actual_child_spawn() {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "hf-root-lease-spawn-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    let scratch = fs::canonicalize(path).unwrap();
    let root = scratch.join("store");
    let store = Store::initialize_fixture(
        &root,
        Limits::default(),
        deadline(),
        hf_platform::lease::shared(),
    )
    .unwrap();
    assert!(matches!(
        Store::recover(&root, deadline(), hf_platform::lease::shared()),
        Err(Error::Busy)
    ));

    let mut child = ChildGuard(
        RetainedOwnedChild::spawn(
            SpawnSpec {
                executable: PathBuf::from("/bin/cat"),
                cwd: fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
                args: Vec::new(),
                environment: Vec::new(),
                capture_limit: 128,
            },
            &MacIdentity,
            deadline(),
        )
        .unwrap(),
    );
    assert_eq!(child.0.initialization(), SpawnInitialization::Ready);
    child.0.close_stdin();
    assert!(matches!(
        child.0.wait_until(deadline(), || false).unwrap(),
        WaitOutcome::Exited(ProcessExit::Code(0))
    ));
    assert!(matches!(
        Store::recover(&root, deadline(), hf_platform::lease::shared()),
        Err(Error::Busy)
    ));
    drop(store);
    fs::remove_dir_all(scratch).unwrap();
}
