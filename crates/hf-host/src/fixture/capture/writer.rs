//! Writer health is permanent; uncertain bytes are never called durable or lost.
use super::*;
use hf_domain::capture::CaptureProgress;
pub(super) struct Writer<'a, P: OwnerPort, G> {
    raw: &'a mut RawCapture,
    session: &'a mut OwnerSession<P>,
    gate: &'a mut G,
    until: Instant,
    append_gate: bool,
    publish_gate: bool,
    /// Accepted FIFO bytes deliberately abandoned after health was lost.
    pub(super) accounting: CaptureAccounting,
}
impl<'a, P: OwnerPort, G: FnMut(Stage) -> io::Result<()>> Writer<'a, P, G> {
    pub fn new(
        raw: &'a mut RawCapture,
        session: &'a mut OwnerSession<P>,
        gate: &'a mut G,
        binding: Option<&CaptureBinding>,
        until: Instant,
    ) -> Self {
        if let Some(binding) = binding {
            if session.begin_capture(binding, until).is_err() {
                session.poison();
            }
        } else {
            session.poison();
        }
        Self {
            raw,
            session,
            gate,
            until,
            append_gate: true,
            publish_gate: true,
            accounting: CaptureAccounting::default(),
        }
    }
    pub fn poisoned(&self) -> bool {
        self.session.is_poisoned()
    }
    pub fn poison(&mut self) {
        self.session.poison();
    }
    pub fn loss(&mut self) {
        if self.poisoned() {
            return;
        }
        if let Some(mut next) = self.session.capture().map(|s| s.progress.clone()) {
            next.loss = true;
            self.publish(next);
        }
        self.poison();
    }
    fn publish(&mut self, mut next: CaptureProgress) -> bool {
        if self.poisoned() {
            return false;
        }
        let Some(revision) = next.revision.checked_add(1) else {
            self.poison();
            return false;
        };
        next.revision = revision;
        if self.session.publish_capture(&next, self.until).is_err() {
            self.poison();
            return false;
        }
        if self.publish_gate {
            self.publish_gate = false;
            if (self.gate)(Stage::CaptureAfterPublish).is_err() {
                self.poison();
            }
        }
        true
    }
    pub fn append(&mut self, stdout: &[u8], stderr: &[u8]) {
        if self.poisoned() {
            self.accounting.abandoned.0 += stdout.len() as u64;
            self.accounting.abandoned.1 += stderr.len() as u64;
            return;
        }
        let Some(mut next) = self.session.capture().map(|s| s.progress.clone()) else {
            self.poison();
            return;
        };
        let gate = &mut self.gate;
        let enabled = self.append_gate;
        let result = self.raw.append(
            next.stdout,
            next.stderr,
            stdout,
            stderr,
            self.until,
            |stage| if enabled { gate(stage) } else { Ok(()) },
        );
        self.append_gate = false;
        match result {
            Ok(offsets) => {
                next.stdout = offsets.stdout;
                next.stderr = offsets.stderr;
                if self.publish(next) {
                    self.accounting.acknowledged.0 += stdout.len() as u64;
                    self.accounting.acknowledged.1 += stderr.len() as u64;
                    return;
                }
            }
            Err(_) => self.poison(),
        }
        self.accounting.uncertain.0 += stdout.len() as u64;
        self.accounting.uncertain.1 += stderr.len() as u64;
    }
    pub fn finish(&mut self, exit: ProcessExit) -> io::Result<()> {
        if self.poisoned() {
            return Err(invalid());
        }
        let Some(mut next) = self.session.capture().map(|s| s.progress.clone()) else {
            self.poison();
            return Err(invalid());
        };
        next.stdout_eof = true;
        next.stderr_eof = true;
        self.publish(next);
        if self.poisoned() {
            return Err(invalid());
        }
        let result = (self.gate)(Stage::CaptureBeforeTerminal)
            .and_then(|()| {
                self.raw.validate(
                    self.session.capture().ok_or_else(invalid)?.progress.stdout,
                    self.session.capture().ok_or_else(invalid)?.progress.stderr,
                    self.until,
                )
            })
            .and_then(|()| map(self.session.finish_capture(exit, self.until)))
            .and_then(|()| (self.gate)(Stage::CaptureAfterTerminal));
        if result.is_err() {
            self.poison();
        }
        result
    }
}
