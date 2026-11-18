//! Original writer service: command dispatch, spawn publication and capture.
use super::{Error, IntentReceiver, WriterEntry, io, stream::Stream, task_projection};
use hf_application::owner_runtime::DigestPort;
use hf_domain::{ownership::PreparedRequest, task_runtime::*};
use hf_interfaces::task_runtime::*;
use hf_platform::task_runtime::raw::{PreparedRawCapture, RawCapture};
use hf_storage::TaskCreate;
pub(super) struct Runtime {
    pub(super) entry: WriterEntry,
    pub(super) intent: LaunchIntentCreate,
    pub(super) raw: Option<RawCapture>,
    prepared: Option<PreparedRawCapture>,
    pub(super) task: Option<TaskCreate>,
    pub(super) stream: Stream,
    pub(super) terminal: bool,
    pub(super) not_started: bool,
}
impl Runtime {
    pub(super) fn new(mut entry: WriterEntry) -> Result<Self, Box<(Error, WriterEntry)>> {
        let initialized = Self::initialize(&mut entry);
        let (intent, prepared, stream) = match initialized {
            Ok(value) => value,
            Err(error) => return Err(Box::new((error, entry))),
        };
        let mut runtime = Self {
            entry,
            intent,
            raw: None,
            prepared: Some(prepared),
            task: None,
            stream,
            terminal: false,
            not_started: false,
        };
        if let Err(error) = runtime.ack() {
            return Err(Box::new((error, runtime.entry)));
        }
        Ok(runtime)
    }
    fn initialize(
        entry: &mut WriterEntry,
    ) -> Result<(LaunchIntentCreate, PreparedRawCapture, Stream), Error> {
        let v = entry.bootstrap().invitation.clone();
        let payload = encode_invitation(&v).map_err(|_| Error::Refused)?;
        let request = PreparedRequest {
            operation: v.binding.operation.clone(),
            session: v.ownership_session.clone(),
            reservation: v.reservation.clone(),
            attempt: v.binding.attempt.clone(),
            generation: v.binding.generation.clone(),
            digest: hf_storage::Sha256Digest.digest_parts(&[&payload]),
            payload,
        };
        let mut transfer = IntentReceiver::new(&request)?;
        loop {
            let frame = io::receive(entry.channel(), io::deadline())?;
            if io::payload(&frame)? == b"HFTI-END\x01" {
                break;
            }
            transfer.push(&frame)?;
        }
        let intent = transfer.finish()?;
        let prepared = RawCapture::prepare(
            entry.hold().root(),
            Box::new(entry.hold().clone()),
            v.binding.clone(),
            io::deadline(),
        )?;
        let stream = Stream::new(&v)?;
        Ok((intent, prepared, stream))
    }
    fn envelope(&mut self) -> Result<WriterEnvelope, Error> {
        let writer = self.entry.bootstrap().writer.clone();
        let e = self.entry.writer().evidence();
        Ok(WriterEnvelope {
            binding: e.binding.clone(),
            owner: e.ownership.owner.clone().ok_or(Error::Refused)?,
            writer,
            revision: e.revision,
            ownership_revision: e.ownership.revision,
        })
    }
    fn ack(&mut self) -> Result<(), Error> {
        let progress = self.raw.as_ref().map(RawCapture::progress);
        let source_binding = self
            .raw
            .as_ref()
            .map(|raw| super::binding::source_binding(raw, &self.entry.bootstrap().invitation));
        let ack = WriterAcknowledgement {
            envelope: self.envelope()?,
            stdout_end: progress.map_or(0, |p| p.stdout_end),
            stderr_end: progress.map_or(0, |p| p.stderr_end),
            close_input: self.stream.close_input(),
            terminal: self.terminal,
            source_binding,
        };
        let bytes = encode_writer_ack(&ack).map_err(|_| Error::Refused)?;
        io::send(self.entry.channel(), io::frame(bytes), io::deadline())
    }
    pub(super) fn command(&mut self, command: WriterCommand) -> Result<(), Error> {
        if command.envelope != self.envelope()? || self.terminal {
            return Err(Error::Refused);
        }
        self.entry.hold().revalidate()?;
        if hf_platform::process::current_parent_identity().as_ref()
            != Some(&command.envelope.owner.process)
        {
            return Err(Error::Refused);
        }
        match command.operation {
            WriterOperation::NotStarted { proof, reason } => {
                if self.task.is_some() || self.raw.is_some() {
                    return Err(Error::Refused);
                }
                let response = super::not_started::writer(&mut self.entry, proof, reason)?;
                let bytes = encode_not_started(&response).map_err(|_| Error::Refused)?;
                io::send(self.entry.channel(), io::frame(bytes), io::deadline())?;
                self.not_started = true;
                return Ok(());
            }
            WriterOperation::Spawn { child, started_at } => self.spawn(child, &started_at)?,
            WriterOperation::Capture {
                stderr,
                offset,
                bytes,
            } => self.capture(stderr, offset, &bytes)?,
            WriterOperation::Finish { proof, finished_at } => self.finish(proof, &finished_at)?,
        }
        self.ack()
    }
    fn spawn(&mut self, child: hf_domain::process::ProcessIdentity, at: &str) -> Result<(), Error> {
        if self.task.is_some() {
            return Err(Error::Refused);
        }
        let metadata = decode_metadata(&self.intent.metadata).map_err(|_| Error::Refused)?;
        let task = task_projection::create(&metadata, self.intent.prompt.clone(), &child, at)?;
        self.entry
            .writer()
            .publish_spawn(&task, &child, io::deadline())?;
        self.raw = Some(self.prepared.take().ok_or(Error::Refused)?.bind(child));
        self.raw
            .as_mut()
            .ok_or(Error::Refused)?
            .view(io::deadline())?;
        self.task = Some(task);
        Ok(())
    }
    fn capture(&mut self, stderr: bool, offset: u64, bytes: &[u8]) -> Result<(), Error> {
        let raw = self.raw.as_mut().ok_or(Error::Refused)?;
        let progress = raw.progress();
        if offset
            != if stderr {
                progress.stderr_end
            } else {
                progress.stdout_end
            }
        {
            return Err(Error::Refused);
        }
        let view = raw.append(
            if stderr { &[] } else { bytes },
            if stderr { bytes } else { &[] },
            io::deadline(),
        )?;
        let mut observation = self.entry.writer().evidence().observation.clone();
        let progress = raw.progress();
        observation.stdout_end = progress.stdout_end;
        observation.stderr_end = progress.stderr_end;
        let events = if !stderr {
            self.stream.consume(bytes, &mut observation)?
        } else {
            Vec::new()
        };
        self.entry
            .writer()
            .publish_capture(&observation, &events, &view, io::deadline())?;
        raw.acknowledge(&view)?;
        Ok(())
    }
}
