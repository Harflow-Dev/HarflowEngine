//! Internal trusted-fixture durable ownership; no owner executable or process verification.
use crate::{
    Error, Result, Store, connection, ownership_codec as codec, ownership_read, ownership_schema,
    ownership_write as write,
};
use hf_domain::{ownership::*, process::ProcessIdentity};
use rusqlite::params;
use std::time::Instant;
impl Store {
    pub fn ownership_prepare(
        &self,
        request: &PreparedRequest,
        deadline: Instant,
    ) -> Result<PrepareResult> {
        codec::validate(request)?;
        self.check_bounds(1, request.payload.len() + 248)?;
        write::generation(self, &request.generation)?;
        write::transaction(self, deadline, |tx| {
            if let Some(existing) = ownership_read::load(self, tx, &request.operation)? {
                return if existing.request == *request {
                    Ok(PrepareResult::Existing(existing))
                } else {
                    Err(Error::OwnershipConflict)
                };
            }
            tx.execute("INSERT INTO ownership_records(operation,session,reservation,attempt,generation,digest,payload,phase,revision,cancelled) VALUES(?1,?2,?3,?4,?5,?6,?7,0,0,0)", params![request.operation.as_str(),request.session.as_str(),request.reservation.as_str(),request.attempt.as_str(),request.generation.as_str(),request.digest.as_str(),request.payload]).map_err(|e| match e { rusqlite::Error::SqliteFailure(ref inner, _) if inner.code == rusqlite::ErrorCode::ConstraintViolation => Error::OwnershipConflict, other => Error::Sqlite(other) })?;
            Ok(PrepareResult::Prepared(Snapshot {
                request: request.clone(),
                phase: Phase::Prepared,
                revision: 0,
                cancelled: false,
                owner: None,
                child: None,
            }))
        })
    }
    pub fn ownership_observe(&self, operation: &Id, deadline: Instant) -> Result<Option<Snapshot>> {
        let mut connection = self.admit(deadline)?;
        let transaction = connection.transaction()?;
        ownership_schema::check(&transaction, &self.identity().generation)?;
        let result = ownership_read::load(self, &transaction, operation)?;
        self.binding.check()?;
        connection::check_deadline(deadline)?;
        Ok(result)
    }
    pub fn ownership_claim(
        &self,
        operation: &Id,
        generation: &Id,
        revision: u64,
        owner: &OwnerClaim,
        deadline: Instant,
    ) -> Result<ClaimResult> {
        write::generation(self, generation)?;
        write::transaction(self, deadline, |tx| {
            let mut snapshot =
                ownership_read::load(self, tx, operation)?.ok_or(Error::OwnershipConflict)?;
            if snapshot.phase != Phase::Prepared {
                return Ok(ClaimResult::NoGrant(snapshot));
            }
            if snapshot.revision != revision {
                return Err(Error::StaleOwnership);
            }
            if snapshot.cancelled {
                return Ok(ClaimResult::NoGrant(snapshot));
            }
            snapshot.phase = Phase::Claimed;
            snapshot.owner = Some(owner.clone());
            codec::next(&mut snapshot)?;
            write::persist(self, tx, &snapshot, revision, None)?;
            Ok(ClaimResult::Granted(snapshot))
        })
    }
    pub fn ownership_cancel(
        &self,
        operation: &Id,
        generation: &Id,
        revision: u64,
        deadline: Instant,
    ) -> Result<Snapshot> {
        write::generation(self, generation)?;
        write::transaction(self, deadline, |tx| {
            let mut snapshot = write::current(self, tx, operation, revision)?;
            if snapshot.phase == Phase::Released {
                return Err(Error::OwnershipConflict);
            }
            if !snapshot.cancelled {
                snapshot.cancelled = true;
                codec::next(&mut snapshot)?;
                write::persist(self, tx, &snapshot, revision, None)?;
            }
            Ok(snapshot)
        })
    }
    pub fn ownership_record_spawn(
        &self,
        fence: &Fence,
        child: &ProcessIdentity,
        deadline: Instant,
    ) -> Result<Snapshot> {
        self.ownership_update(fence, Some(child), deadline)
    }
    pub fn ownership_record_acceptance(
        &self,
        fence: &Fence,
        deadline: Instant,
    ) -> Result<Snapshot> {
        self.ownership_update(fence, None, deadline)
    }
    fn ownership_update(
        &self,
        fence: &Fence,
        child: Option<&ProcessIdentity>,
        deadline: Instant,
    ) -> Result<Snapshot> {
        write::generation(self, &fence.generation)?;
        write::transaction(self, deadline, |tx| {
            let mut snapshot = write::current(self, tx, &fence.operation, fence.revision)?;
            write::fenced(&snapshot, fence)?;
            let desired = match child {
                Some(child)
                    if snapshot.child.as_ref() == Some(child)
                        && matches!(
                            snapshot.phase,
                            Phase::SpawnObserved | Phase::AcceptanceRecorded
                        ) =>
                {
                    return Ok(snapshot);
                }
                Some(child) if snapshot.phase == Phase::Claimed => {
                    snapshot.child = Some(child.clone());
                    Phase::SpawnObserved
                }
                None if snapshot.phase == Phase::AcceptanceRecorded => return Ok(snapshot),
                None if snapshot.phase == Phase::SpawnObserved => Phase::AcceptanceRecorded,
                _ => return Err(Error::OwnershipConflict),
            };
            snapshot.phase = desired;
            codec::next(&mut snapshot)?;
            write::persist(self, tx, &snapshot, fence.revision, None)?;
            Ok(snapshot)
        })
    }
    pub fn ownership_release(
        &self,
        operation: &Id,
        generation: &Id,
        revision: u64,
        fence: Option<&Fence>,
        proof: &ReleaseProof,
        deadline: Instant,
    ) -> Result<Snapshot> {
        write::generation(self, generation)?;
        write::transaction(self, deadline, |tx| {
            let mut snapshot = write::current(self, tx, operation, revision)?;
            if snapshot.phase != Phase::Prepared {
                write::fenced(&snapshot, fence.ok_or(Error::StaleOwnership)?)?;
            } else if fence.is_some() {
                return Err(Error::StaleOwnership);
            }
            let audit = crate::ownership_release::audit(&snapshot, proof)?;
            snapshot.phase = Phase::Released;
            codec::next(&mut snapshot)?;
            write::persist(self, tx, &snapshot, revision, Some(&audit))?;
            Ok(snapshot)
        })
    }
}
