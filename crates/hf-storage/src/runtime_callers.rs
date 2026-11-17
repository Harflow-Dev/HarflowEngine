//! Bounded deny-only original-owner/process evidence, including pre-public-task claims.
use crate::{Error, Result, Store, ownership_codec};
use hf_domain::{
    ownership::{Phase, PreparedRequest},
    process::ProcessIdentity,
};
use std::time::Instant;
impl Store {
    /// Includes retained owners with no public task, including uncertain/live launch claims.
    /// This is deny-only evidence; absence never proves OS ancestry complete.
    pub fn retained_runtime_processes(
        &self,
        limit: usize,
        deadline: Instant,
    ) -> Result<Vec<ProcessIdentity>> {
        if limit == 0 || limit > 256 {
            return Err(Error::LimitExceeded);
        }
        let mut db = self.admit(deadline)?;
        let tx = db.transaction()?;
        let mut statement=tx.prepare("SELECT CASE WHEN owner_process IS NULL THEN NULL WHEN typeof(owner_process)='text' AND length(CAST(owner_process AS BLOB))<=160 THEN owner_process ELSE 1 END,CASE WHEN child IS NULL THEN NULL WHEN typeof(child)='text' AND length(CAST(child AS BLOB))<=160 THEN child ELSE 1 END FROM ownership_records WHERE phase!=4 LIMIT ?1")?;
        let mut rows = statement.query([(limit + 1) as i64])?;
        let mut found = Vec::new();
        let mut count = 0;
        while let Some(row) = rows.next()? {
            crate::connection::check_deadline(deadline)?;
            count += 1;
            if count > limit {
                return Err(Error::LimitExceeded);
            }
            for index in [0, 1] {
                if let Some(value) = row.get::<_, Option<String>>(index)? {
                    found.push(ownership_codec::decode_process(&value)?);
                }
            }
        }
        self.binding.check()?;
        Ok(found)
    }
}
impl Store {
    /// Complete bounded inventory for this Rust root's retained production preparations.
    /// A legacy, malformed, over-budget or undecodable record refuses the whole inventory.
    pub fn runtime_mutation_candidates(
        &self,
        decoder: &dyn hf_application::task_runtime::RuntimeParticipantDecoder,
        limit: usize,
        deadline: Instant,
    ) -> Result<Vec<ProcessIdentity>> {
        self.runtime_candidates(decoder, limit, None, deadline)
    }
    /// Exclude only the exact current Prepared row after trusted composition has validated
    /// its root, immutable intent and live inherited invitation. These bytes alone grant
    /// no authority. Matching process identities in every other operation remain deny evidence.
    pub fn runtime_mutation_candidates_except(
        &self,
        decoder: &dyn hf_application::task_runtime::RuntimeParticipantDecoder,
        limit: usize,
        current: &PreparedRequest,
        deadline: Instant,
    ) -> Result<Vec<ProcessIdentity>> {
        crate::ownership_write::generation(self, &current.generation)?;
        ownership_codec::validate(current)?;
        self.runtime_candidates(decoder, limit, Some(current), deadline)
    }
    fn runtime_candidates(
        &self,
        decoder: &dyn hf_application::task_runtime::RuntimeParticipantDecoder,
        limit: usize,
        exclude: Option<&PreparedRequest>,
        deadline: Instant,
    ) -> Result<Vec<ProcessIdentity>> {
        if limit == 0 || limit > 64 {
            return Err(Error::LimitExceeded);
        }
        let mut db = self.admit(deadline)?;
        let tx = db.transaction()?;
        let mut statement=tx.prepare("SELECT CASE WHEN typeof(operation)='text' AND length(CAST(operation AS BLOB))=32 THEN operation END FROM ownership_records WHERE phase!=4 OR typeof(phase)!='integer' ORDER BY operation LIMIT ?1")?;
        let mut rows = statement.query([(limit + 1) as i64])?;
        let mut result = Vec::new();
        let mut excluded = false;
        let mut count = 0;
        while let Some(row) = rows.next()? {
            crate::connection::check_deadline(deadline)?;
            count += 1;
            if count > limit {
                return Err(Error::LimitExceeded);
            }
            let operation = ownership_codec::id(row.get(0)?)?;
            let record = crate::ownership_read::load(self, &tx, &operation)?
                .ok_or(Error::InvalidOwnership)?;
            let participants = decoder
                .participants(&record.request)
                .map_err(|_| Error::InvalidOwnership)?;
            if participants.len() > 4 {
                return Err(Error::LimitExceeded);
            }
            if exclude.is_some_and(|request| request.operation == record.request.operation) {
                if exclude != Some(&record.request) || record.phase != Phase::Prepared {
                    return Err(Error::StaleOwnership);
                }
                excluded = true;
                continue;
            }
            // The standalone caller is an ordinary parent, never agent evidence. Only
            // a Prepared dispatcher is a pending coordinator; after T1 the retained
            // owner covers every effectful probe and original child ancestry.
            if record.phase == Phase::Prepared {
                result.extend(participants);
            }
            if let Some(owner) = record.owner {
                result.push(owner.process);
            }
            if let Some(child) = record.child {
                result.push(child);
            }
        }
        if exclude.is_some() && !excluded {
            return Err(Error::OwnershipConflict);
        }
        self.binding.check()?;
        crate::connection::check_deadline(deadline)?;
        Ok(result)
    }
}
