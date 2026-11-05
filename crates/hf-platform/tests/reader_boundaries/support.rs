use hf_application::process::IdentityPort;
use hf_domain::{
    ownership::{Digest, Id, OwnerClaim, Phase, PreparedRequest, Snapshot},
    process::ProcessIdentity,
};
use hf_platform::process::{
    CaptureOrigin, MacIdentity, OwnedSignal, RetainedOwnedChild, SignalOutcome, SpawnSpec,
    WaitOutcome,
};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const HELPER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/reader_writer.py"
);
pub const LIMIT_HELPER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/reader_limit.py"
);

pub fn deadline(ms: u64) -> Instant {
    Instant::now() + Duration::from_millis(ms)
}
fn id(c: char) -> Id {
    Id::new(c.to_string().repeat(32)).unwrap()
}
fn request() -> PreparedRequest {
    PreparedRequest {
        operation: id('1'),
        session: id('2'),
        reservation: id('3'),
        attempt: id('4'),
        generation: id('5'),
        digest: Digest::new("6".repeat(64)).unwrap(),
        payload: Vec::new(),
    }
}
pub fn origin(child: &ProcessIdentity) -> CaptureOrigin {
    let owner = OwnerClaim {
        owner: id('7'),
        nonce: id('8'),
        process: MacIdentity.identity(std::process::id()).unwrap(),
    };
    CaptureOrigin::from_spawn_observed(&Snapshot {
        request: request(),
        phase: Phase::SpawnObserved,
        revision: 9,
        cancelled: false,
        owner: Some(owner),
        child: Some(child.clone()),
    })
    .unwrap()
}
pub struct ChildGuard(pub RetainedOwnedChild, pub PathBuf);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.0.close_stdin();
        if matches!(
            self.0.wait_exit_until(deadline(250), || false),
            Ok(WaitOutcome::Exited(_))
        ) {
            let _ = fs::remove_dir_all(&self.1);
            return;
        }
        let Some(identity) = self.0.registered_identity().cloned() else {
            std::process::abort()
        };
        if self
            .0
            .request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(500))
            .ok()
            != Some(SignalOutcome::Sent)
            || !matches!(
                self.0.wait_exit_until(deadline(1_000), || false),
                Ok(WaitOutcome::Exited(_))
            )
        {
            std::process::abort();
        }
        let _ = fs::remove_dir_all(&self.1);
    }
}
pub fn child() -> ChildGuard {
    child_with(HELPER)
}
pub fn child_with(helper: &str) -> ChildGuard {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let scratch = std::env::temp_dir().join(format!(
        "hf-reader-boundary-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir(&scratch).unwrap();
    let witness = scratch.join("written");
    let value = RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: PathBuf::from("/usr/bin/python3"),
            args: vec![
                OsString::from("-I"),
                OsString::from("-S"),
                OsString::from(helper),
                witness.as_os_str().to_owned(),
            ],
            cwd: fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
            environment: Vec::new(),
            capture_limit: 1,
        },
        &MacIdentity,
        deadline(3_000),
    )
    .unwrap();
    ChildGuard(value, scratch)
}
pub fn wait_witness(path: &Path) {
    let until = deadline(6_000);
    while !path.exists() && Instant::now() < until {
        std::thread::yield_now();
    }
    assert!(path.exists(), "fixed helper did not finish writes");
}
