//! Original writer service. Every uncertain operation becomes permanent discard.
use super::{Error, IntentReceiver, WriterEntry, io, stream::Stream, task_projection};
use hf_application::{
    owner_runtime::DigestPort,
    task_runtime::{InvalidIntent, TerminalAdmission},
};
use hf_domain::{
    ownership::PreparedRequest,
    process::ProcessExit,
    task_lifecycle::{ExecutionObservation, LifecycleEvidence},
    task_runtime::*,
};
use hf_interfaces::task_runtime::*;
use hf_platform::task_runtime::raw::{PreparedRawCapture, RawCapture};
use hf_storage::{EventAppend, TaskCreate, TerminalPublication};
use std::time::Duration;
struct Runtime {
    entry: WriterEntry,
    intent: LaunchIntentCreate,
    raw: Option<RawCapture>,
    prepared: Option<PreparedRawCapture>,
    task: Option<TaskCreate>,
    stream: Stream,
    terminal: bool,
    not_started: bool,
}
impl Runtime {
    fn new(mut entry: WriterEntry) -> Result<Self, Box<(Error, WriterEntry)>> {
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
    fn command(&mut self, command: WriterCommand) -> Result<(), Error> {
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
    fn validate_settlement(&mut self, proof: &SettlementAttestation) -> Result<(), Error> {
        if self.entry.writer().evidence().ownership.child.as_ref() != Some(&proof.child)
            || proof.input_bytes
                != encode_live_message(&self.intent.prompt)
                    .map_err(|_| Error::Refused)?
                    .len() as u64
        {
            return Err(Error::Refused);
        }
        if self.raw.as_ref().is_none_or(|raw| {
            super::binding::source_binding(raw, &self.entry.bootstrap().invitation)
                != proof.source_binding
        }) {
            return Err(Error::Refused);
        }
        Ok(())
    }
    fn finish(&mut self, proof: SettlementAttestation, at: &str) -> Result<(), Error> {
        self.validate_settlement(&proof)?;
        let exit = proof.exit;
        let out = proof.stdout_end;
        let err = proof.stderr_end;
        self.stream.finish()?;
        let raw = self.raw.as_mut().ok_or(Error::Refused)?;
        if raw.progress().stdout_end != out || raw.progress().stderr_end != err {
            return Err(Error::Refused);
        }
        let view = raw.view(io::deadline())?;
        let evidence = self.entry.writer().evidence().clone();
        let mut observation = evidence.observation.clone();
        observation.exit = exit;
        observation.drained = true;
        observation.input_closed = true;
        let code = match exit {
            ProcessExit::Code(n) => i64::from(n),
            ProcessExit::Signal(n) => -i64::from(n),
            ProcessExit::Unobserved => return Err(Error::Refused),
        };
        let status = if code == 0 && observation.result.as_ref().is_some_and(|r| r.success) {
            "completed"
        } else {
            "failed"
        };
        let task = self.task.as_mut().ok_or(Error::Refused)?;
        let summary = self.stream.summary().filter(|(_, result)| {
            status == "completed" && evidence.observation.result.as_ref() == Some(*result)
        });
        task_projection::finish(task, status, code, at, summary.map(|(text, _)| text))?;
        let facts = task_projection::terminal_facts(task, code, at);
        let seq = self.stream.next_seq();
        let body = task_finished_observed(&facts, seq, &evidence.observation)
            .map_err(|_| Error::Refused)?;
        let event = EventAppend {
            seq,
            kind: "task_finished".into(),
            raw_len: body.len() as u64,
            body,
        };
        let admission = OwnerAttestation {
            binding: evidence.binding.clone(),
            owner: self.entry.bootstrap().owner.clone(),
            previous: evidence.observation.clone(),
            next: observation.clone(),
            revision: evidence.revision,
            child: proof.child,
        };
        let summary_projection =
            summary.map(|(summary, result)| CanonicalTerminalSummaryProjection { summary, result });
        let projection: &dyn hf_application::task_runtime::TerminalProjectionValidator =
            match &summary_projection {
                Some(projection) => projection,
                None => &CanonicalTerminalProjection,
            };
        let publication = TerminalPublication {
            task,
            observation: &observation,
            event: &event,
            capture: &view,
            settlement: &admission,
            projection,
        };
        self.entry.writer().finish(&publication, io::deadline())?;
        raw.acknowledge(&view)?;
        self.terminal = true;
        Ok(())
    }
}
// Constructed only after a qualified, strictly fenced command from the retained original
// owner. The owner creates Finish exclusively after platform settlement qualification.
struct OwnerAttestation {
    binding: hf_domain::task_lifecycle::LifecycleBinding,
    owner: hf_domain::ownership::OwnerClaim,
    previous: ExecutionObservation,
    next: ExecutionObservation,
    revision: u64,
    child: hf_domain::process::ProcessIdentity,
}
impl TerminalAdmission for OwnerAttestation {
    fn admit(&self, e: &LifecycleEvidence, o: &ExecutionObservation) -> Result<(), InvalidIntent> {
        if e.binding != self.binding
            || e.ownership.owner.as_ref() != Some(&self.owner)
            || e.observation != self.previous
            || o != &self.next
            || e.revision != self.revision
            || e.ownership.child.as_ref() != Some(&self.child)
        {
            return Err(InvalidIntent);
        }
        Ok(())
    }
}
pub fn run_writer() -> i32 {
    let mut entry = match WriterEntry::enter(io::deadline()) {
        Ok(entry) => entry,
        Err(_) => return 1,
    };
    if entry.claim_and_queue_ready(io::deadline()).is_err() {
        return retain_entry(entry);
    }
    loop {
        match entry.flush_ready() {
            Ok(true) => break,
            Ok(false) => std::thread::sleep(Duration::from_millis(1)),
            Err(_) => return retain_entry(entry),
        }
    }
    // Entry ownership must survive all construction failures: initialize in-place below.
    match Runtime::new(entry) {
        Ok(mut runtime) => loop {
            let result = match runtime.entry.channel().receive_step() {
                Ok(hf_platform::task_runtime::channel::Receive::Pending) => {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Ok(hf_platform::task_runtime::channel::Receive::Frame(frame)) => {
                    decode_writer_command(io::payload(&frame).unwrap_or(&[]))
                        .map_err(|_| Error::Refused)
                        .and_then(|command| runtime.command(command))
                }
                Ok(hf_platform::task_runtime::channel::Receive::Eof) => Err(Error::Refused),
                Err(error) => Err(Error::Io(error)),
            };
            if result.is_err() {
                runtime.entry.writer().poison();
                if let Some(raw) = &mut runtime.raw {
                    raw.poison();
                }
                loop {
                    runtime.entry.channel().discard_step();
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            if runtime.terminal || runtime.not_started {
                return 0;
            }
        },
        Err(failure) => {
            let (_, entry) = *failure;
            retain_entry(entry)
        }
    }
}
fn retain_entry(mut entry: WriterEntry) -> i32 {
    entry.writer().poison();
    loop {
        entry.channel().discard_step();
        std::thread::sleep(Duration::from_millis(1));
    }
}
