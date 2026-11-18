//! Writer terminal publication: settlement validation and the finished-task commit.
use super::{Error, io, task_projection, writer_service::Runtime};
use hf_application::task_runtime::{InvalidIntent, TerminalAdmission, TerminalProjectionFacts};
use hf_domain::{
    process::ProcessExit,
    task_lifecycle::{ExecutionObservation, LifecycleEvidence},
    task_runtime::*,
};
use hf_interfaces::task_runtime::*;
use hf_platform::task_runtime::raw::SyncedCaptureView;
use hf_storage::{EventAppend, TerminalPublication};
impl Runtime {
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
    /// Validates the settlement and returns the synced capture view it covers.
    fn settled_view(&mut self, proof: &SettlementAttestation) -> Result<SyncedCaptureView, Error> {
        self.validate_settlement(proof)?;
        self.stream.finish()?;
        let raw = self.raw.as_mut().ok_or(Error::Refused)?;
        if raw.progress().stdout_end != proof.stdout_end
            || raw.progress().stderr_end != proof.stderr_end
        {
            return Err(Error::Refused);
        }
        Ok(raw.view(io::deadline())?)
    }
    pub(super) fn finish(&mut self, proof: SettlementAttestation, at: &str) -> Result<(), Error> {
        let view = self.settled_view(&proof)?;
        let exit = proof.exit;
        let evidence = self.entry.writer().evidence().clone();
        let mut observation = evidence.observation.clone();
        observation.exit = exit;
        observation.drained = true;
        observation.input_closed = true;
        let code = exit_code(exit)?;
        let status = terminal_status(code, &observation);
        let task = self.task.as_mut().ok_or(Error::Refused)?;
        let summary = self.stream.summary().filter(|(_, result)| {
            status == "completed" && evidence.observation.result.as_ref() == Some(*result)
        });
        task_projection::finish(task, status, code, at, summary.map(|(text, _)| text))?;
        let facts = task_projection::terminal_facts(task, code, at);
        let seq = self.stream.next_seq();
        let event = terminal_event(&facts, seq, &evidence.observation)?;
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
        self.raw
            .as_mut()
            .ok_or(Error::Refused)?
            .acknowledge(&view)?;
        self.terminal = true;
        Ok(())
    }
}
fn exit_code(exit: ProcessExit) -> Result<i64, Error> {
    match exit {
        ProcessExit::Code(n) => Ok(i64::from(n)),
        ProcessExit::Signal(n) => Ok(-i64::from(n)),
        ProcessExit::Unobserved => Err(Error::Refused),
    }
}
fn terminal_status(code: i64, observation: &ExecutionObservation) -> &'static str {
    if code == 0 && observation.result.as_ref().is_some_and(|r| r.success) {
        "completed"
    } else {
        "failed"
    }
}
fn terminal_event(
    facts: &TerminalProjectionFacts<'_>,
    seq: u64,
    observation: &ExecutionObservation,
) -> Result<EventAppend, Error> {
    let body = task_finished_observed(facts, seq, observation).map_err(|_| Error::Refused)?;
    Ok(EventAppend {
        seq,
        kind: "task_finished".into(),
        raw_len: body.len() as u64,
        body,
    })
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
