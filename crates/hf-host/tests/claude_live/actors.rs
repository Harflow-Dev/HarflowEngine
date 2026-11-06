#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
use hf_domain::process::ProcessExit;
use hf_platform::process::{MacIdentity, OwnedSignal, RetainedOwnedChild, SpawnInitialization};
use std::{
    ffi::OsString,
    path::Path,
    thread,
    time::{Duration, Instant},
};

pub(super) struct Captured {
    pub(super) exit: ProcessExit,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) identity: String,
    pub(super) complete: bool,
}

/// Sole original waiter; every return and unwind retains active custody until
/// positive wait and physical EOF. Shutdown can remain resident indefinitely.
pub(super) struct Original(pub(super) RetainedOwnedChild);
impl Original {
    pub(super) fn new(mut child: RetainedOwnedChild) -> Self {
        child.close_stdin();
        Self(child)
    }
    pub(super) fn collect(&mut self, until: Instant) -> Captured {
        let mut observed = false;
        if self.0.initialization() == SpawnInitialization::Ready {
            while Instant::now() < until {
                if self.0.pump(until, 8192).is_err() || self.0.try_wait().is_err() {
                    break;
                }
                if settled(&self.0) {
                    observed = true;
                    break;
                }
            }
        }
        // Record failure before cleanup: killing the original never repairs an
        // expired/failed observation into qualification success.
        let complete = observed && capture_clean(&self.0);
        if !observed {
            self.retain_until_settled();
        }
        let identity = format!("{:?}", self.0.registered_identity());
        let exit = self.0.exit();
        let (stdout, stderr) = self.0.take_capture_bytes();
        Captured {
            exit,
            stdout,
            stderr,
            identity,
            complete,
        }
    }
    fn retain_until_settled(&mut self) {
        self.0.close_stdin();
        let _ = self.0.try_wait();
        if self.0.exit() == ProcessExit::Unobserved
            && let Some(identity) = self.0.registered_identity().cloned()
        {
            let _ = self.0.request_signal(
                &identity,
                &MacIdentity,
                OwnedSignal::Kill,
                Instant::now() + Duration::from_secs(2),
            );
        }
        while !settled(&self.0) {
            let _ = self
                .0
                .pump(Instant::now() + Duration::from_millis(20), 8192);
            let _ = self.0.try_wait();
            if !settled(&self.0) {
                thread::sleep(Duration::from_millis(20));
            }
        }
    }
}
impl Drop for Original {
    fn drop(&mut self) {
        if !settled(&self.0) {
            self.retain_until_settled();
        }
    }
}
fn settled(child: &RetainedOwnedChild) -> bool {
    child.exit() != ProcessExit::Unobserved
        && child.capture().stdout.eof
        && child.capture().stderr.eof
}
fn capture_clean(child: &RetainedOwnedChild) -> bool {
    let capture = child.capture();
    !capture.stdout.overflow
        && !capture.stderr.overflow
        && capture.stdout.error.is_none()
        && capture.stderr.error.is_none()
}
pub(super) fn run(
    root: &hf_platform::qualification::FixtureRoot,
    profile: &str,
    executable: &Path,
    args: &[OsString],
    env: Vec<(OsString, OsString)>,
    until: Instant,
    capture_limit: usize,
) -> Captured {
    assert!(
        Instant::now() < until,
        "child observation envelope expired before spawn"
    );
    let _cwd = OriginalCwd::enter(&root.workspace());
    let spec = root
        .spawn_spec(profile, executable, args, env, capture_limit)
        .unwrap();
    let child = RetainedOwnedChild::spawn(
        spec,
        &MacIdentity,
        until.min(Instant::now() + Duration::from_secs(3)),
    )
    .unwrap();
    Original::new(child).collect(until)
}
pub(super) fn require_clean(result: &Captured, phase: &'static str) {
    assert!(
        result.complete,
        "{phase} original child {} observation failed",
        result.identity
    );
    assert_eq!(
        result.exit,
        ProcessExit::Code(0),
        "{phase} original child {} unsuccessful",
        result.identity
    );
}
pub(super) fn is_refusal(result: &Captured) -> bool {
    result.complete && result.exit != ProcessExit::Code(0)
}

/// Created before Original, hence restored only after its child custody ends.
struct OriginalCwd(std::path::PathBuf);
impl OriginalCwd {
    fn enter(path: &Path) -> Self {
        let previous = std::env::current_dir().expect("original controller CWD");
        std::env::set_current_dir(path).expect("owned fixture CWD");
        Self(previous)
    }
}
impl Drop for OriginalCwd {
    fn drop(&mut self) {
        // Do not unwind a second time or let the test exit with changed CWD.
        while std::env::set_current_dir(&self.0).is_err() {
            thread::sleep(Duration::from_millis(20));
        }
    }
}
