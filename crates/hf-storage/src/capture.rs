//! Private trusted fixture capture receipts; callers supply independently observed effects.
use crate::{
    Error, Result, Store, capture_codec as codec, capture_read, capture_schema, connection,
    ownership_read, ownership_schema, ownership_write,
    write_tx::{Publish, Stage},
};
use hf_domain::{
    capture::*,
    ownership::{Id, Phase},
    process::ProcessExit,
};
use rusqlite::{Transaction, params};
use std::time::Instant;
impl Store {
    pub fn capture_begin(
        &self,
        binding: &CaptureBinding,
        deadline: Instant,
    ) -> Result<CaptureSnapshot> {
        let encoded = codec::encode(binding)?;
        self.check_bounds(1, encoded.len() + 104)?;
        self.capture_transaction(deadline, false, |tx| {
            current(self, tx, binding)?;
            if capture_read::load(self, tx, &binding.fence.operation)?.is_some() {
                return Err(Error::OwnershipConflict);
            }
            tx.execute(
                "INSERT INTO capture_records VALUES(?1,?2,0,0,0,0,0,0,0,0)",
                params![binding.fence.operation.as_str(), encoded],
            )?;
            Ok(CaptureSnapshot {
                binding: binding.clone(),
                progress: CaptureProgress {
                    revision: 0,
                    stdout: 0,
                    stderr: 0,
                    stdout_eof: false,
                    stderr_eof: false,
                    loss: false,
                },
                exit: ProcessExit::Unobserved,
            })
        })
    }
    pub fn capture_observe(&self, id: &Id, deadline: Instant) -> Result<Option<CaptureSnapshot>> {
        let mut db = self.admit(deadline)?;
        let tx = db.transaction()?;
        capture_schema::check(&tx, &self.identity().generation)?;
        let result = capture_read::load(self, &tx, id)?;
        self.binding.check()?;
        connection::check_deadline(deadline)?;
        Ok(result)
    }
    pub fn capture_publish(
        &self,
        binding: &CaptureBinding,
        expected_revision: u64,
        next: &CaptureProgress,
        deadline: Instant,
    ) -> Result<CaptureSnapshot> {
        self.capture_transaction(deadline, false, |tx| {
            let mut s = existing(self, tx, binding, expected_revision)?;
            validate_progress(&s.progress, next)?;
            s.progress = next.clone();
            persist(self, tx, &s)?;
            Ok(s)
        })
    }
    pub fn capture_finish(
        &self,
        binding: &CaptureBinding,
        expected_revision: u64,
        exit: ProcessExit,
        deadline: Instant,
    ) -> Result<CaptureSnapshot> {
        if !matches!(exit, ProcessExit::Code(_) | ProcessExit::Signal(1..)) {
            return Err(Error::InvalidOwnership);
        }
        self.capture_transaction(deadline, true, |tx| {
            let mut s = existing(self, tx, binding, expected_revision)?;
            if !s.progress.stdout_eof || !s.progress.stderr_eof || s.progress.loss {
                return Err(Error::OwnershipConflict);
            }
            s.progress.revision = next_revision(expected_revision)?;
            s.exit = exit;
            persist(self, tx, &s)?;
            Ok(s)
        })
    }
    fn capture_transaction<T>(
        &self,
        deadline: Instant,
        terminal: bool,
        action: impl FnOnce(&Transaction<'_>) -> Result<T>,
    ) -> Result<T> {
        let stage = if terminal {
            Stage::CAPTURE_TERMINAL
        } else {
            Stage::CAPTURE
        };
        self.write(deadline, Publish::Nothing, Some(stage), |tx| {
            ownership_schema::check(tx, &self.identity().generation)?;
            capture_schema::check(tx, &self.identity().generation)?;
            action(tx)
        })
    }
}
fn current(store: &Store, tx: &Transaction<'_>, b: &CaptureBinding) -> Result<()> {
    codec::encode(b)?;
    let owner =
        ownership_read::load(store, tx, &b.fence.operation)?.ok_or(Error::OwnershipConflict)?;
    ownership_write::fenced(&owner, &b.fence)?;
    if owner.phase != Phase::AcceptanceRecorded
        || owner.cancelled
        || owner.child.as_ref() != Some(&b.child)
    {
        return Err(Error::StaleOwnership);
    }
    Ok(())
}
fn existing(
    store: &Store,
    tx: &Transaction<'_>,
    b: &CaptureBinding,
    revision: u64,
) -> Result<CaptureSnapshot> {
    current(store, tx, b)?;
    let s = capture_read::load(store, tx, &b.fence.operation)?.ok_or(Error::OwnershipConflict)?;
    if s.binding != *b || s.progress.revision != revision {
        return Err(Error::StaleOwnership);
    }
    if s.exit != ProcessExit::Unobserved {
        return Err(Error::OwnershipConflict);
    }
    Ok(s)
}
fn next_revision(prior: u64) -> Result<u64> {
    prior
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or(Error::LimitExceeded)
}
fn validate_progress(p: &CaptureProgress, n: &CaptureProgress) -> Result<()> {
    if n.revision != next_revision(p.revision)?
        || n.stdout > 65536
        || n.stderr > 65536
        || n.stdout < p.stdout
        || n.stderr < p.stderr
        || (p.stdout_eof && (!n.stdout_eof || n.stdout != p.stdout))
        || (p.stderr_eof && (!n.stderr_eof || n.stderr != p.stderr))
        || (p.loss && !n.loss)
        || (p.stdout == n.stdout
            && p.stderr == n.stderr
            && p.stdout_eof == n.stdout_eof
            && p.stderr_eof == n.stderr_eof
            && p.loss == n.loss)
    {
        return Err(Error::InvalidOwnership);
    }
    Ok(())
}
fn persist(store: &Store, tx: &Transaction<'_>, s: &CaptureSnapshot) -> Result<()> {
    store.check_bounds(2, codec::encode(&s.binding)?.len() + 208)?;
    let id = s.binding.fence.operation.as_str();
    let count: i64 = tx.query_row(
        "SELECT count(*) FROM (SELECT 1 FROM capture_events WHERE operation=?1 LIMIT 129)",
        [id],
        |r| r.get(0),
    )?;
    if count >= 128 {
        return Err(Error::LimitExceeded);
    }
    let (kind, value) = match s.exit {
        ProcessExit::Unobserved => (0, 0),
        ProcessExit::Code(v) => (1, v),
        ProcessExit::Signal(v) => (2, v),
    };
    let p = &s.progress;
    tx.execute("UPDATE capture_records SET revision=?2,stdout=?3,stderr=?4,stdout_eof=?5,stderr_eof=?6,loss=?7,exit_kind=?8,exit_value=?9 WHERE operation=?1",params![id,p.revision as i64,p.stdout as i64,p.stderr as i64,p.stdout_eof,p.stderr_eof,p.loss,kind,value])?;
    tx.execute(
        "INSERT INTO capture_events VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            id,
            p.revision as i64,
            p.stdout as i64,
            p.stderr as i64,
            p.stdout_eof,
            p.stderr_eof,
            p.loss,
            kind,
            value
        ],
    )?;
    Ok(())
}
