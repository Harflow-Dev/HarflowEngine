//! Separate durable acceptance after first public task publication.
use crate::{
    Error, Result, Store, lifecycle_read, ownership_codec, ownership_write,
    write_tx::{Publish, Stage},
};
use hf_application::task_runtime::AcceptanceAdmission;
use hf_domain::{
    ownership::{Fence, Phase},
    task_lifecycle::{LifecycleBinding, LifecycleEvidence},
    task_runtime::AcceptanceReadiness,
};
use rusqlite::params;
use std::time::Instant;
impl Store {
    pub fn record_launch_acceptance(
        &self,
        binding: &LifecycleBinding,
        fence: &Fence,
        expected_revision: u64,
        readiness: &AcceptanceReadiness,
        admission: &dyn AcceptanceAdmission,
        deadline: Instant,
    ) -> Result<LifecycleEvidence> {
        ownership_write::generation(self, &binding.generation)?;
        if readiness.control_endpoint.is_empty()
            || readiness.control_endpoint.len() >= 104
            || !readiness.control_endpoint.starts_with('/')
            || readiness.control_endpoint.chars().any(char::is_control)
            || readiness
                .control_endpoint
                .split('/')
                .skip(1)
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
        {
            return Err(Error::InvalidOwnership);
        }
        self.write(
            deadline,
            Publish::Generation,
            Some(Stage::OWNERSHIP),
            |tx| {
                let mut current =
                    lifecycle_read::load(self, tx, binding)?.ok_or(Error::OwnershipConflict)?;
                ownership_write::fenced(&current.ownership, fence)?;
                if current.revision != expected_revision
                    || current.ownership.phase != Phase::SpawnObserved
                    || current.ownership.child.as_ref() != Some(&readiness.child)
                    || current.cancel.is_some()
                {
                    return Err(Error::StaleOwnership);
                }
                if crate::task_writer::task_valid(tx, &binding.task)? != Some(true) {
                    return Err(Error::TaskMissing);
                }
                admission
                    .admit(&current, readiness)
                    .map_err(|_| Error::InvalidOwnership)?;
                tx.execute(
                    "INSERT INTO lifecycle_acceptance VALUES(?1,?2,?3)",
                    params![
                        binding.task.as_str(),
                        readiness.control_endpoint,
                        readiness.source_binding.as_str()
                    ],
                )?;
                current.ownership.phase = Phase::AcceptanceRecorded;
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
                Ok(current)
            },
        )
    }
}
