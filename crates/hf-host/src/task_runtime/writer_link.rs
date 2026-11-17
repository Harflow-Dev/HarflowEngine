//! Original writer-child custody with bounded bootstrap/ready exchange.
use super::{Error, HeldRoot, OwnerEntry};
use hf_application::process::IdentityPort;
use hf_domain::{
    ownership::OwnerClaim,
    task_runtime::{WriterBootstrap, WriterReady},
};
use hf_interfaces::task_runtime::{decode_writer_ready, encode_writer_bootstrap};
use hf_platform::{
    process::{MacIdentity, RetainedOwnedChild},
    task_runtime::channel::{Channel, DiscardStep, Receive},
};
use std::{fs::File, io::Read, time::Instant};

/// Independent observations of one storage-free discard tick, never settlement proof.
pub struct WriterDrain {
    /// None means endpoints were never transferred; original child custody is retained.
    pub stdout: Option<DiscardStep>,
    pub stderr: StderrDrain,
}
pub enum StderrDrain {
    Unavailable,
    Bytes(usize),
    Pending,
    Eof,
    Error(std::io::Error),
}

pub struct WriterLink {
    child: RetainedOwnedChild,
    hold: HeldRoot,
    expected: Option<WriterBootstrap>,
    channel: Option<Channel>,
    stderr: Option<File>,
    stderr_seen: u64,
    ready: Option<WriterReady>,
    fault: Option<Error>,
}
impl WriterLink {
    /// Always returns custody, including every post-spawn qualification failure.
    /// The supplied child must be the owner's same-release writer child, not a PID.
    pub fn attach(
        entry: &OwnerEntry,
        owner: OwnerClaim,
        child: RetainedOwnedChild,
        deadline: Instant,
    ) -> Self {
        let mut link = Self {
            child,
            hold: entry.hold().clone(),
            expected: None,
            channel: None,
            stderr: None,
            stderr_seen: 0,
            ready: None,
            fault: None,
        };
        if let Err(error) = link.initialize(entry, owner, deadline) {
            link.fault = Some(error);
        }
        link
    }
    fn initialize(
        &mut self,
        entry: &OwnerEntry,
        owner: OwnerClaim,
        deadline: Instant,
    ) -> Result<(), Error> {
        if &owner.process != entry.identity()
            || MacIdentity.identity(std::process::id()).as_ref() != Some(entry.identity())
        {
            return Err(Error::Refused);
        }
        self.hold.revalidate()?;
        let writer = self
            .child
            .registered_identity()
            .cloned()
            .ok_or(Error::Refused)?;
        let chain = hf_platform::task_runtime::caller::observe_caller(writer.pid(), deadline)?;
        if chain.chain.first().map(|link| &link.identity) != Some(&writer)
            || chain.chain.get(1).map(|link| &link.identity) != Some(&owner.process)
        {
            return Err(Error::Refused);
        }
        let path = hf_platform::process::observed_executable_path(&writer, deadline)?;
        let actual = hf_platform::backend_launch::observe_binary_pin(
            &path,
            &hf_storage::Sha256Digest,
            deadline,
        )
        .map_err(|_| Error::Refused)?;
        let mut pin = entry.invitation().ctl.clone();
        pin.logical_path = pin.physical_path.clone();
        if actual != pin {
            return Err(Error::Refused);
        }
        let root = self.hold.root().to_str().ok_or(Error::Refused)?.to_owned();
        let bootstrap = WriterBootstrap {
            root,
            invitation: entry.invitation().clone(),
            owner,
            writer,
        };
        let payload = encode_writer_bootstrap(&bootstrap).map_err(|_| Error::Refused)?;
        let mut packet = (payload.len() as u32).to_be_bytes().to_vec();
        packet.extend_from_slice(&payload);
        let (channel, error) = self.child.take_owner_channel(&bootstrap.writer, 24580)?;
        self.channel = Some(channel);
        self.stderr = Some(error);
        self.expected = Some(bootstrap);
        self.channel.as_mut().ok_or(Error::Refused)?.queue(packet)?;
        Ok(())
    }
    /// One bounded send, receive and stderr read. Fault/expiry permanently switches to
    /// storage-free discard. A cached ready value is historical T1, not current liveness.
    pub fn step(&mut self, deadline: Instant) -> Result<Option<WriterReady>, Error> {
        if self.fault.is_some() || Instant::now() >= deadline {
            self.drain_step();
            return Err(Error::Poisoned);
        }
        let result = self.step_inner();
        // An independent stderr attempt is mandatory even when stdout/protocol failed.
        let stderr = self.drain_stderr();
        match (result, stderr) {
            (Ok(ready), Ok(())) => Ok(ready),
            (Err(error), _) | (_, Err(error)) => {
                self.fault = Some(error);
                Err(Error::Poisoned)
            }
        }
    }
    fn step_inner(&mut self) -> Result<Option<WriterReady>, Error> {
        if let Some(ready) = &self.ready {
            return Ok(Some(ready.clone()));
        }
        self.hold.revalidate()?;
        let expected = self.expected.as_ref().ok_or(Error::Refused)?;
        if MacIdentity.identity(expected.writer.pid()).as_ref() != Some(&expected.writer) {
            return Err(Error::Refused);
        }
        let channel = self.channel.as_mut().ok_or(Error::Refused)?;
        let sent = channel.flush_step()?;
        let packet = match channel.receive_step()? {
            Receive::Pending => return Ok(None),
            Receive::Eof => return Err(Error::Refused),
            Receive::Frame(packet) if sent => packet,
            Receive::Frame(_) => return Err(Error::Refused),
        };
        let ready = decode_writer_ready(packet.get(4..).ok_or(Error::Refused)?)
            .map_err(|_| Error::Refused)?;
        if ready.binding != expected.invitation.binding
            || ready.writer != expected.writer
            || ready.owner != expected.owner
            || ready.revision != 1
            || ready.ownership_revision != 1
        {
            return Err(Error::Refused);
        }
        self.hold.revalidate()?;
        if MacIdentity.identity(expected.writer.pid()).as_ref() != Some(&expected.writer) {
            return Err(Error::Refused);
        }
        self.ready = Some(ready.clone());
        Ok(Some(ready))
    }
    fn drain_stderr(&mut self) -> Result<(), Error> {
        match self.stderr_step() {
            StderrDrain::Error(error) => Err(Error::Io(error)),
            _ => Ok(()),
        }
    }
    fn stderr_step(&mut self) -> StderrDrain {
        let mut bytes = [0; 4096];
        let Some(error) = &mut self.stderr else {
            return StderrDrain::Unavailable;
        };
        match error.read(&mut bytes) {
            Ok(0) => StderrDrain::Eof,
            Ok(count) => {
                self.stderr_seen = self.stderr_seen.saturating_add(count as u64);
                StderrDrain::Bytes(count)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                StderrDrain::Pending
            }
            Err(error) => StderrDrain::Error(error),
        }
    }
    /// Permanently abandons protocol mutation, then attempts one original stdout read
    /// and one original stderr read, each at most 4096 bytes. One error never suppresses
    /// the other attempt. No root/store/identity check, decode, send, reopen or release.
    /// Repeated calls retain the same child, root and descriptors after errors or EOF.
    pub fn drain_step(&mut self) -> WriterDrain {
        if self.fault.is_none() {
            self.fault = Some(Error::Poisoned);
        }
        let stdout = self.channel.as_mut().map(Channel::discard_step);
        let stderr = self.stderr_step();
        WriterDrain { stdout, stderr }
    }
    /// Positively committed historical claim observation; no live/cleanup inference.
    pub fn ready_observation(&self) -> Option<&WriterReady> {
        self.ready.as_ref()
    }
    pub fn is_uncertain(&self) -> bool {
        self.fault.is_some()
    }
    pub fn fault(&self) -> Option<&Error> {
        self.fault.as_ref()
    }
    pub fn child(&mut self) -> &mut RetainedOwnedChild {
        &mut self.child
    }
    pub fn stderr_bytes(&self) -> u64 {
        self.stderr_seen
    }
    pub fn protocol_channel(&mut self) -> Result<&mut Channel, Error> {
        if self.fault.is_some() || self.ready.is_none() {
            return Err(Error::Poisoned);
        }
        self.channel.as_mut().ok_or(Error::Refused)
    }
}
