//! Positive pre-agent refusal never manufactures an ordinary failed task.
use crate::{
    Error, Result, Store, lifecycle_read, ownership_write,
    write_tx::{Publish, Stage},
};
use hf_application::task_runtime::PreAgentRefusalAdmission;
use hf_domain::{
    ownership::{Fence, Phase},
    task_lifecycle::{LifecycleBinding, LifecycleEvidence},
};
use rusqlite::params;
use std::time::Instant;
impl Store {
    pub fn record_launch_not_started(
        &self,
        binding: &LifecycleBinding,
        fence: Option<&Fence>,
        revision: u64,
        reason: &str,
        admission: &dyn PreAgentRefusalAdmission,
        deadline: Instant,
    ) -> Result<LifecycleEvidence> {
        ownership_write::generation(self, &binding.generation)?;
        if reason.is_empty() || reason.len() > 1024 || reason.chars().any(char::is_control) {
            return Err(Error::InvalidOwnership);
        }
        self.write(deadline,Publish::Generation,Some(Stage::OWNERSHIP),|tx|{
            let mut current=lifecycle_read::load(self,tx,binding)?.ok_or(Error::OwnershipConflict)?;
            if current.revision!=revision || current.ownership.child.is_some() || !matches!(current.ownership.phase,Phase::Prepared|Phase::Claimed) || crate::task_writer::task_valid(tx,&binding.task)?.is_some() {return Err(Error::StaleOwnership);}
            if current.ownership.phase==Phase::Claimed {ownership_write::fenced(&current.ownership,fence.ok_or(Error::StaleOwnership)?)?;}else if fence.is_some(){return Err(Error::StaleOwnership);}
            if crate::task_runtime::load(self,tx,binding)?.is_none() {return Err(Error::InvalidOwnership);}
            admission.admit(&current,reason).map_err(|_|Error::InvalidOwnership)?;
            let changed=tx.execute("UPDATE launch_attempts SET state='not_started',not_started_reason=?2 WHERE name=?1 AND state IN('reserved','claimed')",params![binding.task.as_str(),reason])?;
            if changed!=1 {return Err(Error::OwnershipConflict);}
            current.revision=current.revision.checked_add(1).filter(|n|*n<=i64::MAX as u64).ok_or(Error::LimitExceeded)?;
            tx.execute("UPDATE lifecycle_records SET revision=?2 WHERE task=?1",params![binding.task.as_str(),current.revision as i64])?;Ok(current)
        })
    }
}
