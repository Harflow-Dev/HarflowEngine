//! Fresh writer entry and bounded positive T1 acknowledgement; no execution route.
use super::{Error, HeldRoot, Writer};
use hf_application::owner_runtime::DigestPort;
use hf_domain::{
    ownership::PreparedRequest,
    task_runtime::{WriterBootstrap, WriterReady},
};
use hf_interfaces::task_runtime::{
    decode_writer_bootstrap, encode_invitation, encode_writer_ready,
};
use hf_platform::task_runtime::{bootstrap::InheritedStdio, channel::Channel};
use std::{fs::File, path::Path, time::Instant};

pub struct WriterEntry {
    writer: Writer,
    bootstrap: WriterBootstrap,
    channel: Channel,
    _error: File,
    ready_queued: bool,
}
impl WriterEntry {
    /// Construction performs validation only. Caller owns the entry before T1 effects.
    pub fn enter(deadline: Instant) -> Result<Self, Error> {
        let stdio = InheritedStdio::acquire(deadline)?;
        let packet = stdio.read_frame(deadline, 16384)?;
        let bootstrap = decode_writer_bootstrap(packet.get(4..).ok_or(Error::Refused)?)
            .map_err(|_| Error::Refused)?;
        if stdio.observation().process != bootstrap.writer {
            return Err(Error::Refused);
        }
        if bootstrap.owner.owner != bootstrap.invitation.binding.operation
            || bootstrap.owner.nonce != bootstrap.invitation.reservation
        {
            return Err(Error::Refused);
        }
        stdio.qualify(
            &bootstrap.owner.process,
            &bootstrap.invitation.ctl,
            &hf_storage::Sha256Digest,
            deadline,
        )?;
        if std::env::current_dir()? != Path::new(&bootstrap.invitation.cwd) {
            return Err(Error::Refused);
        }
        let hold = HeldRoot::open(Path::new(&bootstrap.root), &bootstrap.invitation, deadline)?;
        let payload = encode_invitation(&bootstrap.invitation).map_err(|_| Error::Refused)?;
        let request = PreparedRequest {
            operation: bootstrap.invitation.binding.operation.clone(),
            session: bootstrap.invitation.ownership_session.clone(),
            reservation: bootstrap.invitation.reservation.clone(),
            attempt: bootstrap.invitation.binding.attempt.clone(),
            generation: bootstrap.invitation.binding.generation.clone(),
            digest: hf_storage::Sha256Digest.digest_parts(&[&payload]),
            payload,
        };
        let writer = Writer::open(hold, &request, &bootstrap.owner, deadline)?;
        stdio.revalidate(deadline)?;
        let (channel, error) = stdio.into_channel(24580)?;
        Ok(Self {
            writer,
            bootstrap,
            channel,
            _error: error,
            ready_queued: false,
        })
    }
    /// Never retry on error: caller retains this entry, including the original root/store.
    pub fn claim_and_queue_ready(&mut self, deadline: Instant) -> Result<(), Error> {
        if self.ready_queued || self.writer.is_poisoned() {
            return Err(Error::Poisoned);
        }
        let ready = WriterReady {
            binding: self.bootstrap.invitation.binding.clone(),
            writer: self.bootstrap.writer.clone(),
            owner: self.bootstrap.owner.clone(),
            revision: 1,
            ownership_revision: 1,
        };
        let payload = encode_writer_ready(&ready).map_err(|_| Error::Refused)?;
        let mut packet = (payload.len() as u32).to_be_bytes().to_vec();
        packet.extend_from_slice(&payload);
        self.writer.claim(self.bootstrap.owner.clone(), deadline)?;
        let evidence = self.writer.evidence();
        if evidence.revision != ready.revision
            || evidence.ownership.revision != ready.ownership_revision
            || evidence.ownership.owner.as_ref() != Some(&ready.owner)
            || evidence.ownership.phase != hf_domain::ownership::Phase::Claimed
        {
            self.writer.poison();
            return Err(Error::Poisoned);
        }
        if let Err(error) = self.channel.queue(packet) {
            self.writer.poison();
            return Err(Error::Io(error));
        }
        self.ready_queued = true;
        Ok(())
    }
    /// A single nonblocking write; true means transport sent, never owner acceptance.
    pub fn flush_ready(&mut self) -> Result<bool, Error> {
        if !self.ready_queued || self.writer.is_poisoned() {
            return Err(Error::Poisoned);
        }
        self.channel.flush_step().map_err(|error| {
            self.writer.poison();
            Error::Io(error)
        })
    }
    pub fn writer(&mut self) -> &mut Writer {
        &mut self.writer
    }
    pub fn channel(&mut self) -> &mut Channel {
        &mut self.channel
    }
    pub fn bootstrap(&self) -> &WriterBootstrap {
        &self.bootstrap
    }
    pub fn hold(&self) -> &HeldRoot {
        &self.writer.hold
    }
}
#[cfg(test)]
impl WriterEntry {
    /// An entry over an already opened writer and an injected channel.
    pub(super) fn for_test(
        writer: Writer,
        bootstrap: WriterBootstrap,
        channel: Channel,
    ) -> Result<Self, Error> {
        Ok(Self {
            writer,
            bootstrap,
            channel,
            _error: File::open("/dev/null")?,
            ready_queued: false,
        })
    }
}
