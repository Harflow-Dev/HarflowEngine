#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_domain::process::ProcessExit;
use hf_platform::process::{
    MacIdentity, OwnedSignal, RetainedOwnedChild, SignalOutcome, WaitOutcome,
};
use std::{
    ops::{Deref, DerefMut},
    time::{Duration, Instant},
};

pub(super) struct ChildGuard {
    child: RetainedOwnedChild,
    settled: bool,
    phase: &'static str,
}
impl ChildGuard {
    pub(super) fn new(child: RetainedOwnedChild, phase: &'static str) -> Self {
        Self {
            child,
            settled: false,
            phase,
        }
    }
    pub(super) fn wait_until(&mut self, deadline: Instant) -> ProcessExit {
        match self.child.wait_until(deadline, || false) {
            Ok(WaitOutcome::Exited(exit @ (ProcessExit::Code(_) | ProcessExit::Signal(_)))) => exit,
            Ok(WaitOutcome::Exited(ProcessExit::Unobserved)) => self.unknown("Exited(Unobserved)"),
            Ok(other) => settle_or_abort(self, other),
            Err(error) => self.unknown(&format!("wait I/O error: {error}")),
        }
    }
    pub(super) fn identity_summary(&self) -> String {
        format!("{:?}", self.child.registered_identity())
    }
    fn unknown(&self, reason: &str) -> ! {
        eprintln!(
            "phase={} child identity={} remains uncertain: {reason}; aborting and retaining fixture",
            self.phase,
            self.identity_summary()
        );
        std::process::abort()
    }
    pub(super) fn settle(&mut self) {
        assert!(self.child.exit() != ProcessExit::Unobserved);
        assert!(self.child.capture().stdout.eof && self.child.capture().stderr.eof);
        self.settled = true;
    }
}
impl Deref for ChildGuard {
    type Target = RetainedOwnedChild;
    fn deref(&self) -> &Self::Target {
        &self.child
    }
}
impl DerefMut for ChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.settled {
            eprintln!(
                "phase={} unsettled Claude child identity={}; aborting fixture",
                self.phase,
                self.identity_summary()
            );
            std::process::abort();
        }
    }
}

pub(super) fn settle_or_abort(child: &mut ChildGuard, outcome: WaitOutcome) -> ! {
    if matches!(outcome, WaitOutcome::Exited(_)) {
        child.unknown("settle_or_abort received an already-observed exit");
    }
    let Some(identity) = child.registered_identity().cloned() else {
        child.unknown("original registered identity is absent");
    };
    let sent = child
        .request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(250))
        .ok();
    let waited = child.child.wait_until(deadline(1000), || false);
    if sent != Some(SignalOutcome::Sent)
        || !matches!(
            waited,
            Ok(WaitOutcome::Exited(
                ProcessExit::Code(_) | ProcessExit::Signal(_)
            ))
        )
    {
        child.unknown(&format!("cleanup after {outcome:?} did not prove exit"));
    }
    child.unknown(&format!(
        "cleanup settled child after {outcome:?}; qualification still fails: {waited:?}"
    ))
}

fn deadline(milliseconds: u64) -> Instant {
    Instant::now() + Duration::from_millis(milliseconds)
}
