#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;

pub(super) fn bounded(overall: Instant, seconds: u64) -> Instant {
    overall.min(Instant::now() + Duration::from_secs(seconds))
}
pub(super) struct ParentGuard<'a> {
    pub(super) child: RetainedOwnedChild,
    pub(super) server: &'a mut server::FixtureServer,
    pub(super) until: Instant,
    pub(super) settled: bool,
}
impl ParentGuard<'_> {
    pub(super) fn child(&mut self) -> &mut RetainedOwnedChild {
        &mut self.child
    }
    pub(super) fn wait_for_post(&mut self, until: Instant) -> Result<(), String> {
        self.server
            .wait_for_validated_post_or_parent_exit(until, &mut self.child)
    }
    pub(super) fn release_post(&mut self) -> Result<(), String> {
        self.server.release_post()
    }
    pub(super) fn mark_settled(&mut self) {
        self.settled = true;
    }
}
impl Drop for ParentGuard<'_> {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let _ = self.server.release_post();
        self.child.close_stdin();
        let mut until = self.until;
        let mut next_error_log = Instant::now();
        loop {
            match self.child.wait_until(until, || false) {
                Ok(WaitOutcome::Exited(exit)) => {
                    eprintln!("phase=qualification-parent-retained-settled exit={exit:?}");
                    while !self.child.capture().stdout.eof || !self.child.capture().stderr.eof {
                        if Instant::now() >= until {
                            eprintln!(
                                "phase=qualification-parent-drain-deadline-overrun retaining-original-pipes=true"
                            );
                            until = Instant::now() + Duration::from_secs(30);
                        }
                        if let Err(error) = self.child.pump(until, 4096) {
                            if Instant::now() >= next_error_log {
                                eprintln!(
                                    "phase=qualification-parent-drain-observation-error error={error:?} retaining-original-pipes=true"
                                );
                                next_error_log = Instant::now() + Duration::from_secs(30);
                            }
                            std::thread::sleep(Duration::from_millis(1));
                            until = Instant::now() + Duration::from_secs(30);
                        }
                    }
                    self.settled = true;
                    return;
                }
                Ok(WaitOutcome::DeadlineExpired) => {
                    eprintln!(
                        "phase=qualification-parent-deadline-overrun retaining-original-waiter=true"
                    );
                    until = Instant::now() + Duration::from_secs(30);
                }
                Ok(WaitOutcome::Cancelled) => continue,
                Err(error) => {
                    if Instant::now() >= next_error_log {
                        eprintln!(
                            "phase=qualification-parent-settlement-unknown error={error:?} retaining-fixture=true"
                        );
                        next_error_log = Instant::now() + Duration::from_secs(30);
                    }
                    std::thread::sleep(Duration::from_millis(1));
                    until = Instant::now() + Duration::from_secs(30);
                }
            }
        }
    }
}

pub(crate) struct Observation {
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) child: hf_domain::process::ProcessIdentity,
}
impl Observation {
    pub(crate) fn require_protocol_witnesses(&self, nonce: &str) {
        assert!(self.stderr.is_empty(), "vendor stderr is empty");
        invocation::require_protocol_witnesses(&self.stdout, nonce);
        eprintln!(
            "phase=owner-hosted-raw child={} stdout-length={} stdout-sha256={} stderr-length={} stderr-sha256={}",
            identity_summary(&self.child),
            self.stdout.len(),
            Sha256Digest.digest_parts(&[&self.stdout]).as_str(),
            self.stderr.len(),
            Sha256Digest.digest_parts(&[&self.stderr]).as_str()
        );
    }
}

pub(super) fn identity_summary(identity: &hf_domain::process::ProcessIdentity) -> String {
    format!(
        "pid={} start={}.{:06} boot={}",
        identity.pid(),
        identity.start_seconds(),
        identity.start_microseconds(),
        identity.boot_uuid()
    )
}

pub(super) fn show(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}
