//! Private T0 and atomic first-public-task T2. No fixture launch grants are accepted here.
use crate::{
    Error, Result, Store, lifecycle_read, ownership_codec, ownership_read, ownership_write,
    task_writer,
    write_tx::{Publish, Stage},
};
use hf_application::task_runtime::{
    InitialIntentValidator, SpawnProjectionFacts, SpawnProjectionValidator,
};
use hf_domain::{ownership::*, process::ProcessIdentity, task_lifecycle::*, task_runtime::*};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest as _, Sha256};
use std::time::Instant;
fn digest(bytes: &[u8]) -> Digest {
    Digest::from_sha256_bytes(Sha256::digest(bytes).into())
}
fn check(intent: &LaunchIntentCreate) -> Result<()> {
    if intent.metadata.len() > 16384 || intent.prompt.len() > 4 * 1024 * 1024 {
        return Err(Error::LimitExceeded);
    }
    if digest(&intent.metadata) != intent.metadata_digest
        || digest(intent.prompt.as_bytes()) != intent.prompt_digest
    {
        return Err(Error::InvalidOwnership);
    }
    Ok(())
}
/// Mandatory observed-spawn facts and trusted canonical projection capability.
pub struct SpawnPublication<'a> {
    pub task: &'a crate::TaskCreate,
    pub child: &'a ProcessIdentity,
    pub projection: &'a dyn SpawnProjectionValidator,
}
impl Store {
    pub fn prepare_launch_intent(
        &self,
        request: &PreparedRequest,
        intent: &LaunchIntentCreate,
        validator: &dyn InitialIntentValidator,
        deadline: Instant,
    ) -> Result<LifecycleEvidence> {
        ownership_codec::validate(request)?;
        ownership_write::generation(self, &request.generation)?;
        check(intent)?;
        validator
            .validate(request, intent)
            .map_err(|_| Error::InvalidOwnership)?;
        let binding = LifecycleBinding {
            task: intent.task.clone(),
            operation: request.operation.clone(),
            attempt: request.attempt.clone(),
            generation: request.generation.clone(),
        };
        self.write(deadline,Publish::Generation,Some(Stage::OWNERSHIP),|tx|{
            crate::ownership_schema::check(tx,&self.identity().generation)?;
            if ownership_read::load(self,tx,&request.operation)?.is_some() || task_writer::task_valid(tx,&intent.task)?.is_some() || tx.query_row("SELECT EXISTS(SELECT 1 FROM lifecycle_records WHERE task=?1 OR attempt=?2) OR EXISTS(SELECT 1 FROM ownership_records WHERE attempt=?2 OR reservation=?3 OR (session=?4 AND phase!=4))",params![intent.task.as_str(),request.attempt.as_str(),request.reservation.as_str(),request.session.as_str()],|r|r.get::<_,bool>(0))? {return Err(Error::OwnershipConflict);}
            tx.execute("INSERT INTO ownership_records(operation,session,reservation,attempt,generation,digest,payload,phase,revision,cancelled) VALUES(?1,?2,?3,?4,?5,?6,?7,0,0,0)",params![request.operation.as_str(),request.session.as_str(),request.reservation.as_str(),request.attempt.as_str(),request.generation.as_str(),request.digest.as_str(),request.payload])?;
            tx.execute("INSERT INTO lifecycle_records VALUES(?1,?2,?3,?4,0,NULL,NULL,NULL,?5)",params![intent.task.as_str(),request.operation.as_str(),request.attempt.as_str(),request.generation.as_str(),crate::lifecycle_codec::encode(&ExecutionObservation::default())])?;
            tx.execute("INSERT INTO lifecycle_intents VALUES(?1,?2,?3,?4,?5,?6)",params![intent.task.as_str(),request.digest.as_str(),intent.metadata,intent.metadata_digest.as_str(),intent.prompt.as_bytes(),intent.prompt_digest.as_str()])?;
            tx.execute("INSERT INTO launch_attempts(name,task_id,generation,state,backend,has_resources,has_worktree) VALUES(?1,?1,?2,'reserved','claude',0,0)",params![intent.task.as_str(),request.generation.as_str()])?;
            lifecycle_read::load(self,tx,&binding)?.ok_or(Error::InvalidOwnership)
        })
    }
    pub fn read_launch_intent(
        &self,
        binding: &LifecycleBinding,
        deadline: Instant,
    ) -> Result<Option<LaunchIntent>> {
        ownership_write::generation(self, &binding.generation)?;
        let mut db = self.admit(deadline)?;
        let tx = db.transaction()?;
        let result = load(self, &tx, binding)?;
        self.binding.check()?;
        crate::connection::check_deadline(deadline)?;
        Ok(result)
    }
    pub fn publish_launch_spawn(
        &self,
        binding: &LifecycleBinding,
        fence: &Fence,
        expected_revision: u64,
        publication: &SpawnPublication<'_>,
        deadline: Instant,
    ) -> Result<LifecycleEvidence> {
        let task = publication.task;
        let child = publication.child;
        let validator = publication.projection;
        ownership_write::generation(self, &binding.generation)?;
        self.write(
            deadline,
            Publish::GenerationAndRevision,
            Some(Stage::OWNERSHIP),
            |tx| {
                let mut current =
                    lifecycle_read::load(self, tx, binding)?.ok_or(Error::OwnershipConflict)?;
                ownership_write::fenced(&current.ownership, fence)?;
                if current.revision != expected_revision
                    || current.ownership.phase != Phase::Claimed
                    || current.cancel.is_some()
                {
                    return Err(Error::StaleOwnership);
                }
                let launch_state: String = tx.query_row(
                    "SELECT state FROM launch_attempts WHERE name=?1",
                    [binding.task.as_str()],
                    |r| r.get(0),
                )?;
                if launch_state != "claimed" {
                    return Err(Error::OwnershipConflict);
                }
                let intent = load(self, tx, binding)?.ok_or(Error::InvalidOwnership)?;
                validate_spawn(&intent, task, child, validator)?;
                task_writer::create_in(tx, task)?;
                current.ownership.child = Some(child.clone());
                current.ownership.phase = Phase::SpawnObserved;
                ownership_codec::next(&mut current.ownership)?;
                ownership_write::persist(self, tx, &current.ownership, fence.revision, None)?;
                current.revision = current
                    .revision
                    .checked_add(1)
                    .filter(|v| *v <= i64::MAX as u64)
                    .ok_or(Error::LimitExceeded)?;
                tx.execute(
                    "UPDATE lifecycle_records SET revision=?2 WHERE task=?1",
                    params![binding.task.as_str(), current.revision as i64],
                )?;
                tx.execute(
                    "UPDATE launch_attempts SET state='spawned' WHERE name=?1",
                    [binding.task.as_str()],
                )?;
                Ok(current)
            },
        )
    }
}
fn validate_spawn(
    intent: &LaunchIntent,
    task: &crate::TaskCreate,
    child: &ProcessIdentity,
    validator: &dyn SpawnProjectionValidator,
) -> Result<()> {
    let f = &task.facts;
    if hf_domain::timestamp::parse_micros(&f.started_at) != Some(f.started_at_us)
        || f.id != intent.binding.task
        || f.status != "running"
        || f.exit_code.is_some()
        || f.finished_at.is_some()
        || f.finished_at_us.is_some()
        || f.output_drained
        || task.prompt != intent.create.prompt
        || task
            .process
            .as_ref()
            .is_none_or(|p| p.pid != child.pid() || p.start.as_ref() != Some(child))
    {
        return Err(Error::InvalidOwnership);
    }
    validator
        .validate(
            intent,
            &SpawnProjectionFacts {
                initial: hf_application::task_lifecycle::InitialProjectionFacts {
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
                child,
            },
            &task.header,
            &task.status,
            &task.prompt,
        )
        .map_err(|_| Error::InvalidOwnership)
}
pub(crate) fn load(
    store: &Store,
    db: &Connection,
    binding: &LifecycleBinding,
) -> Result<Option<LaunchIntent>> {
    let Some(current) = lifecycle_read::load(store, db, binding)? else {
        return Ok(None);
    };
    let valid:Option<bool>=db.query_row("SELECT typeof(metadata)='blob' AND length(metadata)<=16384 AND typeof(prompt)='blob' AND length(prompt)<=4194304 AND typeof(preparation_digest)='text' AND length(preparation_digest)=64 AND typeof(metadata_digest)='text' AND length(metadata_digest)=64 AND typeof(prompt_digest)='text' AND length(prompt_digest)=64 FROM lifecycle_intents WHERE task=?1",[binding.task.as_str()],|r|r.get(0)).optional()?;
    match valid {
        None => return Ok(None),
        Some(false) => return Err(Error::InvalidOwnership),
        Some(true) => {}
    }
    let (preparation,metadata,md,prompt,pd):(String,Vec<u8>,String,Vec<u8>,String)=db.query_row("SELECT preparation_digest,metadata,metadata_digest,prompt,prompt_digest FROM lifecycle_intents WHERE task=?1",[binding.task.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    let preparation_digest =
        Digest::new(preparation.clone()).map_err(|_| Error::InvalidOwnership)?;
    if preparation_digest.as_str() != preparation
        || preparation_digest != current.ownership.request.digest
    {
        return Err(Error::InvalidOwnership);
    }
    let create = LaunchIntentCreate {
        task: binding.task.clone(),
        metadata,
        metadata_digest: Digest::new(md.clone()).map_err(|_| Error::InvalidOwnership)?,
        prompt: String::from_utf8(prompt).map_err(|_| Error::InvalidOwnership)?,
        prompt_digest: Digest::new(pd.clone()).map_err(|_| Error::InvalidOwnership)?,
    };
    if create.metadata_digest.as_str() != md || create.prompt_digest.as_str() != pd {
        return Err(Error::InvalidOwnership);
    }
    check(&create)?;
    Ok(Some(LaunchIntent {
        binding: binding.clone(),
        preparation_digest,
        preparation: current.ownership.request,
        create,
    }))
}
