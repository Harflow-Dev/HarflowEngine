//! Agent spawn, capture-reader start, T2 publication and the Started reply.
use super::{Error, io, owner_runtime::Runtime};
use hf_domain::{ownership::Fence, process::ProcessIdentity, task_launch::*, task_runtime::*};
use hf_interfaces::task_runtime::*;
use hf_platform::process::{CaptureOrigin, MacIdentity, RetainedOwnedChild, SpawnSpec};
use std::{ffi::OsString, time::Duration};
impl Runtime {
    pub(super) fn spawn_agent(
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
        let child = self.spawn_and_capture(spec)?;
        let envelope = self.publish_spawn(&child)?;
        #[cfg(test)]
        super::seams::owner_hit(super::seams::OwnerPoint::AfterT2, self)?;
        self.write_initial_input(&input)?;
        #[cfg(test)]
        super::seams::owner_hit(super::seams::OwnerPoint::BeforeStarted, self)?;
        self.send_started(envelope, child)
    }
    fn spawn_and_capture(&mut self, spec: SpawnSpec) -> Result<ProcessIdentity, Error> {
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
        Ok(child)
    }
    fn publish_spawn(&mut self, child: &ProcessIdentity) -> Result<WriterEnvelope, Error> {
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
        Ok(envelope)
    }
    fn send_started(
        &mut self,
        envelope: WriterEnvelope,
        child: ProcessIdentity,
    ) -> Result<(), Error> {
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
        if (progress.discard && !progress.overflowed)
            || progress.stdout.error.is_some()
            || progress.stderr.error.is_some()
            || self.writer.as_mut().ok_or(Error::Refused)?.try_wait()?
                != hf_domain::process::ProcessExit::Unobserved
        {
            return Err(Error::Refused);
        }
        self.entry.hold().revalidate()?;
        Ok(())
    }
}
