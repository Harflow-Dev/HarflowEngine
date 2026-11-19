//! The owner's retained cause: a private, structured record written once before retention.
//!
//! The record holds the phase, the error kind, the OS error number and the writer's source
//! tag, nothing else: no prompt, output, environment or path. It is written synchronously
//! on the owner thread, best effort, without a retry. A failure only prints one fixed
//! diagnostic line; retention proceeds either way.
use super::{Error, owner_runtime::Runtime, writer_cause::diag};
use hf_platform::task_runtime::raw::cause::write_retained_cause;
use std::{io, time::Duration};

/// How long the owner waits for the writer's tag line before it retains.
const STDERR_BUDGET: Duration = Duration::from_millis(250);
const NOT_RECORDED: &[u8] = b"harflow-owner: retained cause not recorded\n";

/// The owner phase in which it retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Phase {
    Start,
    Tick,
    SettleWriter,
}
impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Tick => "tick",
            Self::SettleWriter => "settle_writer",
        }
    }
}
/// Why the owner retained: the phase and the failure that ended it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RetainCause {
    phase: Phase,
    kind: &'static str,
    errno: Option<i32>,
}
impl RetainCause {
    pub(super) fn new(phase: Phase, error: &Error) -> Self {
        let (kind, errno) = match error {
            Error::Refused => ("refused", None),
            Error::NotStarted(_) => ("not_started", None),
            Error::Poisoned => ("poisoned", None),
            Error::Storage(_) => ("storage", None),
            Error::Io(error) => ("io", error.raw_os_error()),
        };
        Self { phase, kind, errno }
    }
    /// The complete record, ending with the `end` line.
    fn record(self, source: Option<&str>) -> Vec<u8> {
        let errno = self.errno.map_or("-".to_owned(), |n| n.to_string());
        format!(
            "harflow-owner-retained-cause v1\nphase: {}\nkind: {}\nerrno: {}\nsource: {}\nend\n",
            self.phase.name(),
            self.kind,
            errno,
            source.unwrap_or("-")
        )
        .into_bytes()
    }
}
impl Runtime {
    /// Records the cause once, before the retention loop. Capture discard is already
    /// requested; the writer's stderr is collected for a bounded wait; one synchronous
    /// write follows. Nothing here can fail the retention.
    pub(super) fn record_retained_cause(&mut self, cause: RetainCause) {
        debug_assert!(!self.cause_attempted, "the retained cause is written once");
        self.cause_attempted = true;
        // Before the writer exists nothing has created the private payload tree (T1 has not
        // happened): record nothing rather than create it.
        if self.writer.is_none() {
            return;
        }
        let source = self
            .writer
            .as_mut()
            .and_then(|writer| writer.collect_stderr(STDERR_BUDGET));
        if self.write_cause(&cause.record(source)).is_err() {
            diag(NOT_RECORDED);
        }
    }
    fn write_cause(&self, record: &[u8]) -> io::Result<()> {
        let hold = self.entry.hold();
        let (root, identity) = (hold.root(), &hold.source().root);
        let binding = &self.entry.invitation().binding;
        #[cfg(test)]
        match super::seams::cause_write_fault() {
            Some(super::seams::CauseFault::Io) => return Err(io::Error::other("injected")),
            Some(super::seams::CauseFault::ShortWrite(cap)) => {
                return hf_platform::task_runtime::raw::cause::write_retained_cause_capped(
                    root, identity, binding, record, cap,
                );
            }
            None => {}
        }
        write_retained_cause(root, identity, binding, record)
    }
}
