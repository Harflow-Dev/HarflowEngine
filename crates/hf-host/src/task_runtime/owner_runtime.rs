//! Detached original owner: one child waiter, independent reader and original writer.
use super::{Error, IntentReceiver, IntentSender, OwnerEntry, WriterLink, io};
use hf_application::task_launch::LiveInputCodecPort;
use hf_domain::{
    ownership::{Fence, OwnerClaim},
    task_launch::*,
    task_runtime::*,
};
use hf_interfaces::task_runtime::*;
use hf_platform::{
    backend_launch::PathClaudeDiscovery,
    process::{CaptureOrigin, CaptureReader, MacIdentity, RetainedOwnedChild, SpawnSpec},
};
use std::{ffi::OsString, path::PathBuf, time::Duration};
struct Codec;
impl LiveInputCodecPort for Codec {
    fn encode_user_line(&self, prompt: &str) -> Result<Vec<u8>, LaunchRefusal> {
        encode_live_message(prompt).map_err(|_| LaunchRefusal::Codec)
    }
}
pub(super) struct Runtime {
    pub(super) entry: OwnerEntry,
    pub(super) writer: Option<WriterLink>,
    pub(super) agent: Option<RetainedOwnedChild>,
    pub(super) reader: Option<CaptureReader>,
    pub(super) discovery: Option<PathClaudeDiscovery<hf_storage::Sha256Digest>>,
    pub(super) envelope: Option<WriterEnvelope>,
    pub(super) origin: Option<CaptureOrigin>,
    pub(super) intent: Option<LaunchIntentCreate>,
    pub(super) input_bytes: u64,
    pub(super) stdout_end: u64,
    pub(super) stderr_end: u64,
    pub(super) input_closed: bool,
    pub(super) source_binding: Option<hf_domain::ownership::Digest>,
}
impl Runtime {
    fn new(entry: OwnerEntry) -> Self {
        Self {
            entry,
            writer: None,
            agent: None,
            reader: None,
            discovery: None,
            envelope: None,
            origin: None,
            intent: None,
            input_bytes: 0,
            stdout_end: 0,
            stderr_end: 0,
            input_closed: false,
            source_binding: None,
        }
    }
    fn initialize(&mut self) -> Result<(), Error> {
        let mut receiver = IntentReceiver::new(self.entry.request())?;
        loop {
            let frame = io::receive(self.entry.channel(), io::deadline())?;
            if io::payload(&frame)? == b"HFTI-END\x01" {
                break;
            }
            receiver.push(&frame)?;
        }
        let intent = receiver.finish()?;
        let v = self.entry.invitation().clone();
        let environment =
            super::environment::receive_environment(self.entry.channel(), &v.environment_digest)?;
        if v.configuration_digest != super::configuration_digest()
            || v.invocation_digest != super::invocation_digest(&v.session)
            || v.expected_version != hf_backends::backend_launch::CLAUDE_VERSION
        {
            return Err(Error::Refused);
        }
        self.intent = Some(intent.clone());
        self.start_writer(&intent)?;
        // T1 and raw readiness are positively acknowledged before any effectful probe.
        let environment: Vec<_> = environment
            .into_iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect();
        self.discovery = Some(PathClaudeDiscovery::new_pinned(
            hf_storage::Sha256Digest,
            environment.clone(),
            v.backend.clone(),
        ));
        let spec = LaunchSpec {
            session_id: v.session.clone(),
            prompt: intent.prompt,
            cwd: v.cwd.clone(),
            permission: LaunchPermission::Unrestricted,
            options: Vec::new(),
        };
        let preparation = hf_backends::backend_launch::prepare_claude(
            &spec,
            self.discovery.as_mut().ok_or(Error::Refused)?,
            &Codec,
            io::deadline(),
        );
        let prepared = match preparation {
            Ok(value) => value,
            Err(LaunchRefusal::UnsupportedVersion) => return super::not_started::owner(self),
            Err(_) => return Err(Error::Refused),
        };
        if prepared.executable.binary != v.backend
            || prepared.executable.version != v.expected_version
            || prepared.args != super::invocation_arguments(&v.session)
        {
            return Err(Error::Refused);
        }
        self.spawn_agent(prepared, environment)?;
        Ok(())
    }
    fn start_writer(&mut self, intent: &LaunchIntentCreate) -> Result<(), Error> {
        let v = self.entry.invitation();
        let claim = OwnerClaim {
            owner: v.binding.operation.clone(),
            nonce: v.reservation.clone(),
            process: self.entry.identity().clone(),
        };
        let spec = SpawnSpec {
            executable: v.ctl.physical_path.clone().into(),
            cwd: v.cwd.clone().into(),
            args: vec!["__task-writer".into()],
            environment: Vec::new(),
            capture_limit: 16384,
        };
        let child = RetainedOwnedChild::spawn_in_owner_group(spec, &MacIdentity, io::deadline())?;
        self.writer = Some(WriterLink::attach(
            &self.entry,
            claim,
            child,
            io::deadline(),
        ));
        let deadline = io::deadline();
        let ready = loop {
            if let Some(ready) = self.writer.as_mut().ok_or(Error::Refused)?.step(deadline)? {
                break ready;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        self.envelope = Some(WriterEnvelope {
            binding: ready.binding,
            owner: ready.owner,
            writer: ready.writer,
            revision: ready.revision,
            ownership_revision: ready.ownership_revision,
        });
        let mut sender = IntentSender::new(intent.clone())?;
        while let Some(frame) = sender.next_frame()? {
            io::send(self.writer_channel()?, frame, io::deadline())?;
        }
        io::send(
            self.writer_channel()?,
            io::frame(b"HFTI-END\x01".to_vec()),
            io::deadline(),
        )?;
        let ack = self.receive_ack()?;
        if ack.stdout_end != 0 || ack.stderr_end != 0 || ack.close_input || ack.terminal {
            return Err(Error::Refused);
        }
        Ok(())
    }
    fn spawn_agent(
        &mut self,
        prepared: PreparedInvocation,
        environment: Vec<(OsString, OsString)>,
    ) -> Result<(), Error> {
        let input = prepared.initial_input;
        let spec = SpawnSpec {
            executable: prepared.executable.binary.physical_path.into(),
            cwd: prepared.cwd.into(),
            args: prepared.args.into_iter().map(OsString::from).collect(),
            environment,
            capture_limit: 0,
        };
        self.agent = Some(RetainedOwnedChild::spawn_in_owner_group(
            spec,
            &MacIdentity,
            io::deadline(),
        )?);
        let child = self
            .agent
            .as_ref()
            .and_then(RetainedOwnedChild::registered_identity)
            .cloned()
            .ok_or(Error::Refused)?;
        let envelope = self.envelope.as_ref().ok_or(Error::Refused)?.clone();
        // Anticipated tag grants drainage only; exact T2 acknowledgement is required
        // before these bytes may be published or a Started response constructed.
        let origin = CaptureOrigin {
            child: child.clone(),
            fence: Fence {
                operation: envelope.binding.operation.clone(),
                generation: envelope.binding.generation.clone(),
                attempt: envelope.binding.attempt.clone(),
                owner: envelope.owner.clone(),
                revision: envelope.ownership_revision + 1,
            },
        };
        self.origin = Some(origin.clone());
        self.reader = Some(
            self.agent
                .as_mut()
                .ok_or(Error::Refused)?
                .start_capture_reader(origin, None)?,
        );
        self.exchange(WriterOperation::Spawn {
            child: child.clone(),
            started_at: super::timestamp()?,
        })?;
        let envelope = self.envelope.as_ref().ok_or(Error::Refused)?.clone();
        if self
            .origin
            .as_ref()
            .is_none_or(|origin| origin.fence.revision != envelope.ownership_revision)
        {
            return Err(Error::Refused);
        }
        self.write_initial_input(&input)?;
        let bytes = encode_started(&Started {
            envelope,
            child,
            source_binding: self.source_binding.clone().ok_or(Error::Refused)?,
        })
        .map_err(|_| Error::Refused)?;
        io::send(self.entry.channel(), io::frame(bytes), io::deadline())?;
        Ok(())
    }
    fn write_initial_input(&mut self, input: &[u8]) -> Result<(), Error> {
        let deadline = io::deadline();
        while self.input_bytes < input.len() as u64 {
            let count = self
                .agent
                .as_mut()
                .ok_or(Error::Refused)?
                .write_stdin(&input[self.input_bytes as usize..], deadline)?;
            self.input_bytes += count as u64;
            if std::time::Instant::now() >= deadline {
                return Err(Error::Poisoned);
            }
            if count == 0 {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let progress = self
            .reader
            .as_ref()
            .ok_or(Error::Refused)?
            .view()
            .snapshot();
        if progress.discard
            || progress.stdout.error.is_some()
            || progress.stderr.error.is_some()
            || self
                .writer
                .as_mut()
                .ok_or(Error::Refused)?
                .child()
                .try_wait()?
                != hf_domain::process::ProcessExit::Unobserved
        {
            return Err(Error::Refused);
        }
        self.entry.hold().revalidate()?;
        Ok(())
    }
    pub(super) fn writer_channel(
        &mut self,
    ) -> Result<&mut hf_platform::task_runtime::channel::Channel, Error> {
        self.writer
            .as_mut()
            .ok_or(Error::Refused)?
            .protocol_channel()
    }
    fn receive_ack(&mut self) -> Result<WriterAcknowledgement, Error> {
        let frame = io::receive(self.writer_channel()?, io::deadline())?;
        let ack = decode_writer_ack(io::payload(&frame)?).map_err(|_| Error::Refused)?;
        let expected = self.envelope.as_ref().ok_or(Error::Refused)?;
        if &ack.envelope != expected {
            return Err(Error::Refused);
        }
        if self
            .source_binding
            .as_ref()
            .is_some_and(|binding| ack.source_binding.as_ref() != Some(binding))
        {
            return Err(Error::Refused);
        }
        if self.source_binding.is_none() {
            self.source_binding = ack.source_binding.clone();
        }
        Ok(ack)
    }
    pub(super) fn exchange(
        &mut self,
        operation: WriterOperation,
    ) -> Result<WriterAcknowledgement, Error> {
        let current = self.envelope.as_ref().ok_or(Error::Refused)?.clone();
        let mut expected_out = self.stdout_end;
        let mut expected_err = self.stderr_end;
        if let WriterOperation::Capture {
            stderr,
            offset,
            bytes,
        } = &operation
        {
            let expected = if *stderr {
                &mut expected_err
            } else {
                &mut expected_out
            };
            if *offset != *expected {
                return Err(Error::Refused);
            }
            *expected = expected
                .checked_add(bytes.len() as u64)
                .ok_or(Error::Refused)?;
        }
        let terminal = matches!(operation, WriterOperation::Finish { .. });
        let ownership_increment = u64::from(matches!(operation, WriterOperation::Spawn { .. }));
        let command = WriterCommand {
            envelope: current.clone(),
            operation,
        };
        let bytes = encode_writer_command(&command).map_err(|_| Error::Refused)?;
        io::send(self.writer_channel()?, io::frame(bytes), io::deadline())?;
        let mut next = current;
        next.revision += 1;
        next.ownership_revision += ownership_increment;
        self.envelope = Some(next);
        let ack = self.receive_ack()?;
        if ack.stdout_end != expected_out
            || ack.stderr_end != expected_err
            || ack.terminal != terminal
            || ack.source_binding.is_none()
        {
            return Err(Error::Refused);
        }
        self.stdout_end = ack.stdout_end;
        self.stderr_end = ack.stderr_end;
        if ack.close_input && !self.input_closed {
            self.agent.as_mut().ok_or(Error::Refused)?.close_stdin();
            self.input_closed = true;
        }
        Ok(ack)
    }
}

pub fn run_owner(root: PathBuf) -> i32 {
    let entry = match OwnerEntry::enter(&root, io::deadline()) {
        Ok(entry) => entry,
        Err(_) => return 1,
    };
    let mut runtime = Runtime::new(entry);
    match runtime.initialize() {
        Ok(()) => (),
        Err(Error::NotStarted(_)) => return 0,
        Err(_) => runtime.retain_uncertain(),
    }
    loop {
        match runtime.tick() {
            Ok(true) => break,
            Ok(false) => std::thread::sleep(Duration::from_millis(1)),
            Err(_) => runtime.retain_uncertain(),
        }
    }
    if runtime.settle_writer().is_err() {
        runtime.retain_uncertain();
    }
    0
}
