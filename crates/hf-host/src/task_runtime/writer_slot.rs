//! The owner's writer custody: the writer child process.
use super::{Error, OwnerEntry, WriterDrain, WriterLink, io};
use hf_domain::{ownership::OwnerClaim, process::ProcessExit, task_runtime::WriterReady};
use hf_platform::{
    process::{MacIdentity, RetainedOwnedChild, SpawnSpec},
    task_runtime::channel::Channel,
};
use std::time::Instant;
pub(super) enum WriterSlot {
    Link(Box<WriterLink>),
    #[cfg(test)]
    Thread(Box<super::tests::thread_writer::ThreadWriter>),
}
impl WriterSlot {
    /// Spawns the writer child of the owner's group and attaches it.
    pub(super) fn start(entry: &OwnerEntry, claim: OwnerClaim) -> Result<Self, Error> {
        #[cfg(test)]
        if let Some(slot) = super::seams::writer_slot(entry, &claim) {
            return slot;
        }
        let spec = SpawnSpec {
            executable: entry.invitation().ctl.physical_path.clone().into(),
            cwd: entry.invitation().cwd.clone().into(),
            args: vec!["__task-writer".into()],
            environment: Vec::new(),
            capture_limit: 16384,
        };
        let child = RetainedOwnedChild::spawn_in_owner_group(spec, &MacIdentity, io::deadline())?;
        Ok(Self::Link(Box::new(WriterLink::attach(
            entry,
            claim,
            child,
            io::deadline(),
        ))))
    }
    pub(super) fn step(&mut self, deadline: Instant) -> Result<Option<WriterReady>, Error> {
        match self {
            Self::Link(link) => link.step(deadline),
            #[cfg(test)]
            Self::Thread(thread) => thread.step(deadline),
        }
    }
    pub(super) fn protocol_channel(&mut self) -> Result<&mut Channel, Error> {
        match self {
            Self::Link(link) => link.protocol_channel(),
            #[cfg(test)]
            Self::Thread(thread) => thread.protocol_channel(),
        }
    }
    /// The writer's exit: `Unobserved` while it runs.
    pub(super) fn try_wait(&mut self) -> std::io::Result<ProcessExit> {
        match self {
            Self::Link(link) => link.child().try_wait(),
            #[cfg(test)]
            Self::Thread(thread) => thread.try_wait(),
        }
    }
    /// The writer's source tag from its stderr, within a bounded wait.
    pub(super) fn collect_stderr(&mut self, budget: std::time::Duration) -> Option<&'static str> {
        match self {
            Self::Link(link) => link.collect_stderr(budget),
            #[cfg(test)]
            Self::Thread(thread) => thread.collect_stderr(),
        }
    }
    pub(super) fn drain_step(&mut self) -> WriterDrain {
        match self {
            Self::Link(link) => link.drain_step(),
            #[cfg(test)]
            Self::Thread(thread) => thread.drain_step(),
        }
    }
}
