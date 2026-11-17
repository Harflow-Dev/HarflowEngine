//! Atomic lifecycle evidence; terminal public projection deliberately remains closed.
use crate::{
    Error, Result, Store, lifecycle_codec as evidence,
    lifecycle_invariants::validate_observation,
    lifecycle_read::load,
    ownership_codec as codec, ownership_read, ownership_write as ownership,
    task_writer::{TaskCreate, create_in},
    write_tx::{Publish, Stage},
};
use hf_domain::{ownership::*, task_lifecycle::*};
use rusqlite::params;
use std::time::Instant;
/// Event publication requires a trusted capture capability; there is no default adapter.
pub struct LifecycleEventPublication<'a> {
    pub events: &'a [crate::EventAppend],
    pub capture: &'a dyn hf_application::task_lifecycle::DurableCaptureAdmission,
}
impl Store {
    /// Closed legacy foundation only; actual public launches use prepare_launch_intent.
    /// Creates one reservation, launch association and initial task in the same commit.
    /// Existing identities refuse rather than replay an uncertain preparation.
    pub fn prepare_lifecycle(
        &self,
        request: &PreparedRequest,
        task: &TaskCreate,
        projection: &dyn hf_application::task_lifecycle::InitialProjectionValidator,
        deadline: Instant,
    ) -> Result<LifecycleEvidence> {
        codec::validate(request)?;
        ownership::generation(self, &request.generation)?;
        if !matches!(task.facts.status.as_str(), "queued" | "running")
            || task.process.is_some()
            || task.facts.exit_code.is_some()
            || task.facts.finished_at.is_some()
            || task.facts.output_drained
        {
            return Err(Error::InvalidOwnership);
        }
        let f = &task.facts;
        projection
            .validate(
                &hf_application::task_lifecycle::InitialProjectionFacts {
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
                &task.header,
                &task.status,
            )
            .map_err(|_| Error::InvalidOwnership)?;
        self.check_bounds(
            1,
            request.payload.len()
                + task.prompt.len()
                + task.header.len()
                + task.status.len()
                + 4096,
        )?;
        let binding = LifecycleBinding {
            task: task.facts.id.clone(),
            operation: request.operation.clone(),
            attempt: request.attempt.clone(),
            generation: request.generation.clone(),
        };
        self.write(deadline,Publish::GenerationAndRevision,Some(Stage::OWNERSHIP),|tx|{
            crate::ownership_schema::check(tx,&self.identity().generation)?;
            if ownership_read::load(self,tx,&request.operation)?.is_some(){return Err(Error::OwnershipConflict);}
            tx.execute("INSERT INTO ownership_records(operation,session,reservation,attempt,generation,digest,payload,phase,revision,cancelled) VALUES(?1,?2,?3,?4,?5,?6,?7,0,0,0)",params![request.operation.as_str(),request.session.as_str(),request.reservation.as_str(),request.attempt.as_str(),request.generation.as_str(),request.digest.as_str(),request.payload])?;
            create_in(tx,task)?;
            tx.execute("INSERT INTO launch_attempts(name,task_id,generation,state,backend,has_resources,has_worktree) VALUES(?1,?1,?2,'reserved',?3,0,0)",params![binding.task.as_str(),binding.generation.as_str(),task.facts.backend])?;
            tx.execute("INSERT INTO lifecycle_records VALUES(?1,?2,?3,?4,0,NULL,NULL,NULL,?5)",params![binding.task.as_str(),binding.operation.as_str(),binding.attempt.as_str(),binding.generation.as_str(),evidence::encode(&ExecutionObservation::default())])?;
            load(self,tx,&binding)?.ok_or(Error::InvalidOwnership)
        })
    }
    /// CAS revision is lifecycle-local; fence revision is the unchanged ownership protocol.
    pub fn apply_lifecycle(
        &self,
        binding: &LifecycleBinding,
        fence: Option<&Fence>,
        expected_revision: u64,
        delta: &LifecycleDelta,
        deadline: Instant,
    ) -> Result<LifecycleEvidence> {
        self.apply_lifecycle_write(binding, fence, expected_revision, delta, None, deadline)
    }
    /// Requires trusted capture admission before atomic event/source publication.
    /// This seam has no production capture implementation and does not qualify public T3.
    pub fn apply_lifecycle_events(
        &self,
        binding: &LifecycleBinding,
        fence: Option<&Fence>,
        expected_revision: u64,
        delta: &LifecycleDelta,
        publication: &LifecycleEventPublication<'_>,
        deadline: Instant,
    ) -> Result<LifecycleEvidence> {
        self.apply_lifecycle_write(
            binding,
            fence,
            expected_revision,
            delta,
            Some(publication),
            deadline,
        )
    }
    fn apply_lifecycle_write(
        &self,
        binding: &LifecycleBinding,
        fence: Option<&Fence>,
        expected_revision: u64,
        delta: &LifecycleDelta,
        publication: Option<&LifecycleEventPublication<'_>>,
        deadline: Instant,
    ) -> Result<LifecycleEvidence> {
        let events = publication.map_or(&[][..], |p| p.events);
        ownership::generation(self, &binding.generation)?;
        if !events.is_empty() && !matches!(delta, LifecycleDelta::ObservationCommitted(_)) {
            return Err(Error::InvalidEventQuery);
        }
        if events.len() > 64 {
            return Err(Error::LimitExceeded);
        }
        for event in events {
            crate::event_writer::check_event(event)?;
            if event.raw_len != event.body.len() as u64
                || event.body.len() > crate::EVENT_BODY_LIMIT
                || event.kind == "task_finished"
            {
                return Err(Error::InvalidEventQuery);
            }
        }
        self.write(deadline,Publish::Generation,Some(Stage::OWNERSHIP),|tx|{
            let mut current=load(self,tx,binding)?.ok_or(Error::OwnershipConflict)?;
            if tx.query_row("SELECT EXISTS(SELECT 1 FROM lifecycle_intents i JOIN launch_attempts l ON l.name=i.task WHERE i.task=?1 AND l.state='not_started')",[binding.task.as_str()],|r|r.get::<_,bool>(0))? {return Err(Error::OwnershipConflict);}

            if tx.query_row("SELECT EXISTS(SELECT 1 FROM lifecycle_intents i JOIN task_facts f ON f.id=i.task WHERE i.task=?1 AND f.status IN('completed','failed','cancelled'))",[binding.task.as_str()],|r|r.get::<_,bool>(0))? {return Err(Error::OwnershipConflict);}
            if current.revision!=expected_revision{return Err(Error::StaleOwnership);}
            if !events.is_empty() {
                let LifecycleDelta::ObservationCommitted(next)=delta else{return Err(Error::InvalidEventQuery);};
                if current.ownership.child.is_none() || next.stdout_end<=current.observation.stdout_end && next.stderr_end<=current.observation.stderr_end {return Err(Error::InvalidEventQuery);}
            }
            if matches!(delta,LifecycleDelta::ChildRecorded(_)) && tx.query_row("SELECT EXISTS(SELECT 1 FROM lifecycle_intents WHERE task=?1)",[binding.task.as_str()],|r|r.get::<_,bool>(0))? {return Err(Error::InvalidOwnership);}
            let previous=current.observation.clone();
            apply(&mut current,fence,delta)?;
            admit_capture(&current,&previous,publication)?;
            let appended=events.iter().try_fold(0u64,|sum,event|sum.checked_add(event.raw_len+1).ok_or(Error::LimitExceeded))?;
            current.observation.normalized_event_bytes=current.observation.normalized_event_bytes.checked_add(appended).filter(|v|*v<=64*1024*1024-65_536).ok_or(Error::LimitExceeded)?;
            if !events.is_empty(){
                let facts=hf_domain::event_read::LogFacts{task_status:None,available:true,capture:hf_domain::event_read::CaptureState::Live,output_drained:None,eof_finished_seq:None,within_grace:false};
                crate::event_writer::append_in(tx,&binding.task,events,&facts,deadline)?;
            }
            if current.ownership.revision!=ownership_read::load(self,tx,&binding.operation)?.ok_or(Error::OwnershipConflict)?.revision {
                let prior=fence.map_or(0,|f|f.revision);ownership::persist(self,tx,&current.ownership,prior,None)?;
            }
            current.revision=current.revision.checked_add(1).filter(|v|*v<=i64::MAX as u64).ok_or(Error::LimitExceeded)?;
            let cancel=current.cancel.as_ref();
            tx.execute("UPDATE lifecycle_records SET revision=?2,cancel_seq=?3,cancel_reason=?4,cancel_at=?5,observation=?6 WHERE task=?1",params![binding.task.as_str(),current.revision as i64,cancel.map(|c|c.sequence as i64),cancel.map(|c|c.reason.as_str()),cancel.map(|c|c.accepted_at.as_str()),evidence::encode(&current.observation)])?;
            // Private intent/claim/cancel evidence must not invalidate the public task watch.
            if crate::task_writer::task_valid(tx,&binding.task)?.is_some() {crate::watch_meta::publish(tx)?;}
            let state=if current.ownership.child.is_some(){"spawned"}else if current.ownership.owner.is_some(){"claimed"}else{"reserved"};
            tx.execute("UPDATE launch_attempts SET state=?2 WHERE name=?1",params![binding.task.as_str(),state])?;
            Ok(current)
        })
    }
    pub fn read_lifecycle(
        &self,
        binding: &LifecycleBinding,
        deadline: Instant,
    ) -> Result<Option<LifecycleEvidence>> {
        ownership::generation(self, &binding.generation)?;
        let mut db = self.admit(deadline)?;
        let tx = db.transaction()?;
        let result = load(self, &tx, binding)?;
        self.binding.check()?;
        crate::connection::check_deadline(deadline)?;
        Ok(result)
    }
}
fn apply(
    current: &mut LifecycleEvidence,
    fence: Option<&Fence>,
    delta: &LifecycleDelta,
) -> Result<()> {
    if current.ownership.phase == Phase::Released {
        return Err(Error::OwnershipConflict);
    }
    if let LifecycleDelta::OwnerClaimed(owner) = delta {
        if fence.is_some() || current.ownership.phase != Phase::Prepared || current.cancel.is_some()
        {
            return Err(Error::OwnershipConflict);
        }
        current.ownership.owner = Some(owner.clone());
        current.ownership.phase = Phase::Claimed;
        codec::next(&mut current.ownership)?;
        return Ok(());
    }
    ownership::fenced(&current.ownership, fence.ok_or(Error::StaleOwnership)?)?;
    match delta {
        LifecycleDelta::ChildRecorded(child)
            if current.ownership.phase == Phase::Claimed && current.cancel.is_none() =>
        {
            current.ownership.child = Some(child.clone());
            current.ownership.phase = Phase::SpawnObserved;
            codec::next(&mut current.ownership)?;
        }
        LifecycleDelta::CancelCommitted(intent) => {
            if intent.sequence == 0
                || intent.sequence > i64::MAX as u64
                || intent.reason.len() > 1024
                || intent.accepted_at.len() > 128
            {
                return Err(Error::LimitExceeded);
            }
            if let Some(previous) = &current.cancel {
                if previous != intent {
                    return Err(Error::OwnershipConflict);
                }
            } else {
                current.cancel = Some(intent.clone());
                current.ownership.cancelled = true;
                codec::next(&mut current.ownership)?;
            }
        }
        LifecycleDelta::ObservationCommitted(next) | LifecycleDelta::ExecutionObserved(next)
            if current.ownership.child.is_some() =>
        {
            validate_observation(&current.observation, next)?;
            current.observation = next.clone();
        }
        _ => return Err(Error::OwnershipConflict),
    }
    Ok(())
}
fn admit_capture(
    current: &LifecycleEvidence,
    previous: &ExecutionObservation,
    publication: Option<&LifecycleEventPublication<'_>>,
) -> Result<()> {
    use hf_application::task_lifecycle::{CaptureProgress, DurableCaptureRequest};
    let next = &current.observation;
    let progress = |o: &ExecutionObservation| CaptureProgress {
        stdout_end: o.stdout_end,
        stderr_end: o.stderr_end,
        known_loss: o.capture_known_loss,
    };
    if progress(previous) == progress(next) && publication.is_none_or(|p| p.events.is_empty()) {
        return Ok(());
    }
    let admission = publication.ok_or(Error::InvalidOwnership)?;
    let child = current
        .ownership
        .child
        .as_ref()
        .ok_or(Error::InvalidOwnership)?;
    admission
        .capture
        .admit(&DurableCaptureRequest {
            binding: &current.binding,
            child,
            previous: progress(previous),
            next: progress(next),
        })
        .map_err(|_| Error::InvalidOwnership)
}
