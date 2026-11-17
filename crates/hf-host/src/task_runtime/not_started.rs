//! Positive pre-agent refusal over the original authenticated owner/writer channel.
use super::{Error, WriterEntry, io, owner_runtime::Runtime};
use hf_application::task_runtime::{InvalidIntent, PreAgentRefusalAdmission};
use hf_domain::{ownership::Phase, task_lifecycle::LifecycleEvidence, task_runtime::*};
use hf_interfaces::task_runtime::*;
pub(super) fn writer(
    entry: &mut WriterEntry,
    proof: ProbeAttestation,
    reason: String,
) -> Result<NotStarted, Error> {
    let v = &entry.bootstrap().invitation;
    if reason != "unsupported_version"
        || proof.backend_digest != v.backend.digest
        || proof.exit != hf_domain::process::ProcessExit::Code(0)
        || proof.stdout_bytes > 4096
        || proof.stderr_bytes != 0
        || proof.child == entry.bootstrap().owner.process
        || proof.child == entry.bootstrap().writer
    {
        return Err(Error::Refused);
    }
    let previous = entry.writer().evidence().clone();
    if previous.ownership.phase != Phase::Claimed || previous.ownership.child.is_some() {
        return Err(Error::Refused);
    }
    let admission = RemoteRefusal {
        previous,
        reason: reason.clone(),
    };
    entry
        .writer()
        .not_started(&reason, &admission, io::deadline())?;
    let writer = entry.bootstrap().writer.clone();
    let evidence = entry.writer().evidence();
    let envelope = WriterEnvelope {
        binding: evidence.binding.clone(),
        owner: evidence.ownership.owner.clone().ok_or(Error::Refused)?,
        writer,
        revision: evidence.revision,
        ownership_revision: evidence.ownership.revision,
    };
    Ok(NotStarted {
        envelope,
        reason,
        proof,
    })
}
struct RemoteRefusal {
    previous: LifecycleEvidence,
    reason: String,
}
impl PreAgentRefusalAdmission for RemoteRefusal {
    fn admit(&self, e: &LifecycleEvidence, reason: &str) -> Result<(), InvalidIntent> {
        if e != &self.previous
            || reason != self.reason
            || e.ownership.child.is_some()
            || e.ownership.phase != Phase::Claimed
        {
            return Err(InvalidIntent);
        }
        Ok(())
    }
}
pub(super) fn owner(runtime: &mut Runtime) -> Result<(), Error> {
    if runtime.agent.is_some() || runtime.reader.is_some() || runtime.origin.is_some() {
        return Err(Error::Refused);
    }
    let operation = runtime
        .discovery
        .as_ref()
        .and_then(|d| d.settled_probe())
        .ok_or(Error::Refused)?
        .unsupported_version_operation();
    let WriterOperation::NotStarted { proof, reason } = &operation else {
        return Err(Error::Refused);
    };
    let proof = proof.clone();
    let reason = reason.clone();
    let mut expected = runtime.envelope.clone().ok_or(Error::Refused)?;
    let bytes = encode_writer_command(&WriterCommand {
        envelope: expected.clone(),
        operation,
    })
    .map_err(|_| Error::Refused)?;
    io::send(runtime.writer_channel()?, io::frame(bytes), io::deadline())?;
    let frame = io::receive(runtime.writer_channel()?, io::deadline())?;
    let response = decode_not_started(io::payload(&frame)?).map_err(|_| Error::Refused)?;
    expected.revision += 1;
    if response.envelope != expected || response.proof != proof || response.reason != reason {
        return Err(Error::Refused);
    }
    runtime.envelope = Some(expected);
    runtime.entry.hold().revalidate()?;
    runtime.settle_writer()?;
    io::send(runtime.entry.channel(), frame, io::deadline())?;
    Err(Error::NotStarted(reason))
}
