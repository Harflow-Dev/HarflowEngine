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

impl Store {
    /// Complete deny-only ordinary active-process inventory for mutating run admission.
    /// No ancestry/session filter can hide an unqualified row. Only a valid terminal
    /// fact with an integer exit is excluded; missing or legacy identity refuses all.
    pub fn ordinary_runtime_mutation_candidates(
        &self,
        limit: usize,
        deadline: Instant,
    ) -> Result<Vec<ProcessIdentity>> {
        if limit == 0 || limit > 64 {
            return Err(Error::LimitExceeded);
        }
        let mut db = self.admit(deadline)?;
        let tx = db.transaction()?;
        let context = self.read_context();
        if crate::TaskSnapshot::new(&context, &tx, deadline).damaged()? {
            return Err(Error::MalformedRow);
        }
        let mut statement = tx.prepare(ORDINARY_RUNTIME_INVENTORY)?;
        let mut rows = statement.query([(limit + 1) as i64])?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            crate::connection::check_deadline(deadline)?;
            if result.len() == limit {
                return Err(Error::LimitExceeded);
            }
            result.push(ordinary_runtime_identity(row)?);
        }
        self.binding.check()?;
        crate::connection::check_deadline(deadline)?;
        Ok(result)
    }
}
// CASE prevents copying unbounded variable-length data before the host-side checks.
const ORDINARY_RUNTIME_INVENTORY: &str = "SELECT \
 CASE WHEN typeof(task)='text' AND length(CAST(task AS BLOB)) BETWEEN 1 AND 64 THEN task END,\
 CASE WHEN typeof(status)='text' AND length(CAST(status AS BLOB)) BETWEEN 1 AND 256 THEN status END,\
 CASE WHEN typeof(boot)='text' AND length(CAST(boot AS BLOB)) BETWEEN 1 AND 64 THEN boot END,\
 pid,seconds,micros,valid,exit_kind FROM (\
 SELECT f.id AS task,f.status AS status,p.boot_uuid AS boot,p.pid AS pid,\
 p.start_seconds AS seconds,p.start_micros AS micros,f.valid AS valid,typeof(f.exit_code) AS exit_kind \
 FROM task_facts f LEFT JOIN task_processes p ON p.task=f.id \
 WHERE NOT (typeof(f.valid)='integer' AND f.valid=1 AND typeof(f.status)='text' \
 AND f.status IN ('completed','failed','timed_out','cancelled') AND typeof(f.exit_code)='integer') \
 UNION ALL \
 SELECT p.task,NULL,p.boot_uuid,p.pid,p.start_seconds,p.start_micros,NULL,'null' \
 FROM task_processes p LEFT JOIN task_facts f ON f.id=p.task WHERE f.id IS NULL\
 ) LIMIT ?1";
fn ordinary_runtime_identity(row: &rusqlite::Row<'_>) -> Result<ProcessIdentity> {
    let task: String = row.get(0)?;
    hf_domain::task_read::TaskId::new(task).map_err(|_| Error::MalformedRow)?;
    let _status: String = row.get(1)?;
    let boot: String = row.get(2)?;
    if row.get::<_, i64>(6)? != 1 {
        return Err(Error::MalformedRow);
    }
    let exit_kind: String = row.get(7)?;
    if !matches!(exit_kind.as_str(), "null" | "integer") {
        return Err(Error::MalformedRow);
    }
    let pid = u32::try_from(row.get::<_, i64>(3)?).map_err(|_| Error::MalformedRow)?;
    let seconds = u64::try_from(row.get::<_, i64>(4)?).map_err(|_| Error::MalformedRow)?;
    let micros = u64::try_from(row.get::<_, i64>(5)?).map_err(|_| Error::MalformedRow)?;
    ProcessIdentity::new(pid, seconds, micros, boot).map_err(|_| Error::MalformedRow)
}
