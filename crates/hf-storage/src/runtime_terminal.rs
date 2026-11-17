//! Typed final execution projection. This grants no workflow settlement or release.
use crate::{
    Error, EventAppend, Result, Store, TaskCreate, lifecycle_read, ownership_write,
    write_tx::{Publish, Stage},
};
use hf_application::{
    task_lifecycle::{
        CaptureProgress, DurableCaptureAdmission, DurableCaptureRequest, InitialProjectionFacts,
    },
    task_runtime::{TerminalAdmission, TerminalProjectionFacts, TerminalProjectionValidator},
};
use hf_domain::{
    ownership::{Fence, Phase},
    process::ProcessExit,
    task_lifecycle::*,
};
use rusqlite::params;
use std::time::Instant;
pub struct TerminalPublication<'a> {
    pub task: &'a TaskCreate,
    pub observation: &'a ExecutionObservation,
    pub event: &'a EventAppend,
    pub capture: &'a dyn DurableCaptureAdmission,
    pub settlement: &'a dyn TerminalAdmission,
    pub projection: &'a dyn TerminalProjectionValidator,
}
impl Store {
    pub fn publish_task_terminal(
        &self,
        binding: &LifecycleBinding,
        fence: &Fence,
        expected_revision: u64,
        p: &TerminalPublication<'_>,
        deadline: Instant,
    ) -> Result<LifecycleEvidence> {
        ownership_write::generation(self, &binding.generation)?;
        crate::event_writer::check_event(p.event)?;
        if p.event.kind != "task_finished"
            || p.event.body.len() > 16384
            || p.event.raw_len != p.event.body.len() as u64
        {
            return Err(Error::InvalidEventQuery);
        }
        self.write(deadline,Publish::GenerationAndRevision,Some(Stage::OWNERSHIP),|tx|{
            let mut current=lifecycle_read::load(self,tx,binding)?.ok_or(Error::OwnershipConflict)?;
            ownership_write::fenced(&current.ownership,fence)?;
            if current.revision!=expected_revision || !matches!(current.ownership.phase,Phase::SpawnObserved|Phase::AcceptanceRecorded) {return Err(Error::StaleOwnership);}
            validate(&current,p)?;
            let progress=|o:&ExecutionObservation|CaptureProgress {stdout_end:o.stdout_end,stderr_end:o.stderr_end,known_loss:o.capture_known_loss};
            p.capture.admit(&DurableCaptureRequest {binding,child:current.ownership.child.as_ref().ok_or(Error::InvalidOwnership)?,previous:progress(&current.observation),next:progress(p.observation)}).map_err(|_|Error::InvalidOwnership)?;
            p.settlement.admit(&current,p.observation).map_err(|_|Error::InvalidOwnership)?;
            let f=&p.task.facts;
            let intent=crate::task_runtime::load(self,tx,binding)?.ok_or(Error::InvalidOwnership)?;
            if p.task.prompt!=intent.create.prompt {return Err(Error::InvalidOwnership);}
            let immutable:(Option<String>,Option<String>,Option<String>,Option<String>)=tx.query_row("SELECT title,model,continuation_of,chain_root FROM task_facts WHERE id=?1 AND valid=1",[binding.task.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
            if immutable!=(f.title.clone(),f.model.clone(),f.continuation_of.as_ref().map(|v|v.as_str().to_owned()),f.chain_root.clone()) {return Err(Error::InvalidOwnership);}

            let existing:(String,String,String,String,i64)=tx.query_row("SELECT backend,repo_path,session_id,started_at,started_at_us FROM task_facts WHERE id=?1 AND valid=1 AND status='running'",[binding.task.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
            if existing!=(f.backend.clone(),f.repo_path.clone(),f.session_id.clone(),f.started_at.clone(),f.started_at_us) {return Err(Error::InvalidOwnership);}
            crate::event_writer::append_in(tx,&binding.task,std::slice::from_ref(p.event),&hf_domain::event_read::LogFacts {task_status:Some(f.status.clone()),available:true,capture:hf_domain::event_read::CaptureState::ObservedTerminal,output_drained:Some(true),eof_finished_seq:Some(p.event.seq),within_grace:false},deadline)?;
            tx.execute("UPDATE event_streams SET task_status=?2,capture=2,drained=1,eof_seq=?3,grace=0 WHERE task=?1",params![binding.task.as_str(),f.status,p.event.seq as i64])?;
            tx.execute("UPDATE task_facts SET status=?2,exit_code=?3,finished_at=?4,finished_at_us=?5,output_drained=1 WHERE id=?1",params![binding.task.as_str(),f.status,f.exit_code,f.finished_at,f.finished_at_us])?;
            tx.execute("UPDATE task_headers SET active=0,recorded_status=?2,exited=1,drained=1,header=?3,status=?4 WHERE id=?1",params![binding.task.as_str(),f.status,p.task.header,p.task.status])?;
            current.observation=p.observation.clone();
            current.observation.normalized_event_bytes=current.observation.normalized_event_bytes.checked_add(p.event.raw_len+1).filter(|n|*n<=64*1024*1024-65_536).ok_or(Error::LimitExceeded)?;
            current.revision=current.revision.checked_add(1).filter(|n|*n<=i64::MAX as u64).ok_or(Error::LimitExceeded)?;
            tx.execute("UPDATE lifecycle_records SET revision=?2,observation=?3 WHERE task=?1",params![binding.task.as_str(),current.revision as i64,crate::lifecycle_codec::encode(&current.observation)])?;
            Ok(current)
        })
    }
}
fn validate(current: &LifecycleEvidence, p: &TerminalPublication<'_>) -> Result<()> {
    crate::lifecycle_invariants::validate_observation(&current.observation, p.observation)?;
    let o = p.observation;
    let f = &p.task.facts;
    let exit = match o.exit {
        ProcessExit::Code(code) => i64::from(code),
        ProcessExit::Signal(signal) => -i64::from(signal),
        ProcessExit::Unobserved => return Err(Error::InvalidOwnership),
    };
    let status = if current.cancel.is_some() {
        "cancelled"
    } else if exit == 0 && o.result.as_ref().is_some_and(|r| r.success) {
        "completed"
    } else {
        "failed"
    };
    if f.finished_at
        .as_deref()
        .and_then(hf_domain::timestamp::parse_micros)
        != f.finished_at_us
        || f.finished_at_us.is_some_and(|n| n < f.started_at_us)
        || !o.drained
        || !o.input_closed
        || f.id != current.binding.task
        || f.status != status
        || f.exit_code != Some(exit)
        || f.finished_at.is_none()
        || f.finished_at_us.is_none()
        || !f.output_drained
        || p.task.header.len() > crate::TASK_PROJECTION_BYTES
        || p.task.status.len() > crate::TASK_PROJECTION_BYTES
        || p.task.process.as_ref().is_none_or(|p| {
            p.start.as_ref() != current.ownership.child.as_ref()
                || current
                    .ownership
                    .child
                    .as_ref()
                    .is_none_or(|c| c.pid() != p.pid)
        })
    {
        return Err(Error::InvalidOwnership);
    }
    p.projection
        .validate(
            current,
            &TerminalProjectionFacts {
                initial: InitialProjectionFacts {
                    task: &f.id,
                    backend: &f.backend,
                    repo_path: &f.repo_path,
                    session_id: &f.session_id,
                    status: &f.status,
                    started_at: &f.started_at,
                    title: f.title.as_deref(),
                    model: f.model.as_deref(),
                    continuation_of: f.continuation_of.as_ref(),
                    chain_root: f.chain_root.as_deref(),
                },
                exit_code: exit,
                finished_at: f.finished_at.as_deref().ok_or(Error::InvalidOwnership)?,
                output_drained: true,
            },
            &p.task.header,
            &p.task.status,
            &p.event.body,
            p.event.seq,
        )
        .map_err(|_| Error::InvalidOwnership)
}
