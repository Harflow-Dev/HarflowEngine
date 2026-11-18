//! Owner start phase: intent and invitation admission, writer start, launch preparation.
use super::{Error, IntentReceiver, IntentSender, WriterLink, io, owner_runtime::Runtime};
use hf_application::task_launch::LiveInputCodecPort;
use hf_domain::{ownership::OwnerClaim, task_launch::*, task_runtime::*};
use hf_interfaces::task_runtime::*;
use hf_platform::{
    backend_launch::PathClaudeDiscovery,
    process::{MacIdentity, RetainedOwnedChild, SpawnSpec},
};
use std::{ffi::OsString, time::Duration};
struct Codec;
impl LiveInputCodecPort for Codec {
    fn encode_user_line(&self, prompt: &str) -> Result<Vec<u8>, LaunchRefusal> {
        encode_live_message(prompt).map_err(|_| LaunchRefusal::Codec)
    }
}
impl Runtime {
    pub(super) fn initialize(&mut self) -> Result<(), Error> {
        let intent = self.receive_intent()?;
        let v = self.entry.invitation().clone();
        let environment = self.admit_environment()?;
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
    fn receive_intent(&mut self) -> Result<LaunchIntentCreate, Error> {
        let mut receiver = IntentReceiver::new(self.entry.request())?;
        loop {
            let frame = io::receive(self.entry.channel(), io::deadline())?;
            if io::payload(&frame)? == b"HFTI-END\x01" {
                break;
            }
            receiver.push(&frame)?;
        }
        receiver.finish()
    }
    fn admit_environment(&mut self) -> Result<Vec<(String, String)>, Error> {
        let v = self.entry.invitation().clone();
        let environment =
            super::environment::receive_environment(self.entry.channel(), &v.environment_digest)?;
        if v.configuration_digest != super::configuration_digest()
            || v.invocation_digest != super::invocation_digest(&v.session)
            || v.expected_version != hf_backends::backend_launch::CLAUDE_VERSION
        {
            return Err(Error::Refused);
        }
        Ok(environment)
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
}
