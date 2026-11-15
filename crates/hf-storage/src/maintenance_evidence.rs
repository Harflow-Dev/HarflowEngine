//! Owner evidence of the root a maintenance session holds: the survey decides the basis
//! and where the rows are read from, and the rows are read read-only with checkpoints
//! suppressed. With `-wal` missing the source is never opened (SQLite would create an
//! empty `-wal` in it): the main file is read from a private copy.
use crate::{
    Error, Identity, Result,
    binding::Binding,
    connection,
    maintenance::MaintenanceSession,
    maintenance_copy::{Scratch, copy_main},
    maintenance_rows,
    read_session::open_read_only,
    survey::{self, Check, Outcome, Survey},
    watermark,
};
use hf_application::ctl::state_recover::evidence::{
    Basis, COPY_BUDGET, EvidenceError, EvidenceSet, Item, OwnerEvidencePort, QUERY_BUDGET,
};
use hf_domain::ctl::state_recovery::is_operation_id;
use rusqlite::{Connection, OpenFlags};
use std::time::{Duration, Instant};

/// The deadlines of the evidence read.
#[derive(Clone, Copy, Debug)]
pub struct EvidenceBounds {
    /// The private copy of the main file.
    pub copy: Duration,
    /// The SQLite queries (after the copy, when there is one).
    pub queries: Duration,
}
impl Default for EvidenceBounds {
    fn default() -> Self {
        Self {
            copy: COPY_BUDGET,
            queries: QUERY_BUDGET,
        }
    }
}

/// What the survey says about the store before any row is read.
struct Plan {
    basis: Basis,
    identity: Identity,
    mark: Option<u64>,
    /// `-wal` is missing: read a private copy, never the source.
    copy: bool,
}

impl MaintenanceSession {
    /// Reads the owner evidence of this root for `operation_id` (32 lowercase hex digits,
    /// which names the scratch directory). A verdict about the data is `Ok`, including
    /// `Unreadable`, which also covers a failed copy (scratch, free space, deadline) and
    /// a binding failure. An error means the root is no longer the one the session
    /// acquired (`Replaced`; nothing is read from the replacement) or a scratch that
    /// cannot be removed (an I/O error, never `Replaced`).
    pub fn evidence(&self, operation_id: &str, bounds: EvidenceBounds) -> Result<EvidenceSet> {
        if !is_operation_id(operation_id) {
            return Err(Error::InvalidRoot);
        }
        let unreadable = || EvidenceSet {
            basis: Basis::Unreadable,
            items: Vec::new(),
        };
        let binding = match self.bind() {
            Ok(binding) => binding,
            Err(Error::Replaced) => return Err(Error::Replaced),
            Err(_) => return Ok(unreadable()),
        };
        let survey = survey::maintenance(&binding, &self.authority);
        let Some(plan) = plan(&survey) else {
            self.verify()?;
            return Ok(unreadable());
        };
        let (read, cleanup) = if plan.copy {
            self.read_copy(&binding, &plan, operation_id, bounds)
        } else {
            let deadline = Instant::now() + bounds.queries;
            (read_open(&binding, &plan, deadline), Ok(()))
        };
        cleanup.map_err(|error| {
            Error::Io(std::io::Error::other(format!(
                "evidence scratch cleanup: {error}"
            )))
        })?;
        self.verify()?;
        Ok(match read {
            Ok((behind, items)) => EvidenceSet {
                basis: if behind {
                    Basis::EarlierSnapshot
                } else {
                    plan.basis
                },
                items,
            },
            Err(_) => unreadable(),
        })
    }

    /// The copy path: the scratch is removed before this returns, and its removal failing
    /// is reported separately from the read.
    fn read_copy(
        &self,
        binding: &Binding,
        plan: &Plan,
        operation_id: &str,
        bounds: EvidenceBounds,
    ) -> (Result<(bool, Vec<Item>)>, Result<()>) {
        let mut scratch = match Scratch::create(&self.root, operation_id) {
            Ok(scratch) => scratch,
            Err(error) => return (Err(error), Ok(())),
        };
        let read = self.read_scratch(binding, plan, &mut scratch, bounds);
        (read, scratch.remove())
    }

    fn read_scratch(
        &self,
        binding: &Binding,
        plan: &Plan,
        scratch: &mut Scratch,
        bounds: EvidenceBounds,
    ) -> Result<(bool, Vec<Item>)> {
        let parent = self.root.parent().ok_or(Error::InvalidRoot)?;
        let copy_deadline = Instant::now() + bounds.copy;
        let output = scratch.create_copy()?;
        copy_main(&self.root, output, parent, self.fs.as_ref(), copy_deadline)?;
        connection::check_deadline(copy_deadline)?;
        let deadline = Instant::now() + bounds.queries;
        // Sidecars are adopted only once SQLite has opened the copy and closed it again;
        // a failure before the open records nothing, so a foreign file is never deleted.
        let connection = Connection::open_with_flags(
            scratch.database(),
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let rows = connection::suppress_checkpoints(&connection)
            .and_then(|()| read_rows(&connection, plan, deadline));
        drop(connection);
        scratch.snapshot()?;
        let rows = rows?;
        binding.check()?;
        Ok(rows)
    }
}

impl OwnerEvidencePort for MaintenanceSession {
    fn read(&self, operation_id: &str) -> std::result::Result<EvidenceSet, EvidenceError> {
        self.evidence(operation_id, EvidenceBounds::default())
            .map_err(|error| match error {
                Error::Replaced => EvidenceError::Stale,
                _ => EvidenceError::Unavailable,
            })
    }
}

/// The basis the checks imply and whether the rows can be read at all; `None` when they
/// cannot (sidecar, marker, header, journal or binding failure, or a failure of another
/// kind in a state file).
fn plan(survey: &Survey) -> Option<Plan> {
    let passed = |check| matches!(survey.outcome(check), Outcome::Passed);
    if ![
        Check::Sidecars,
        Check::Marker,
        Check::Header,
        Check::Journal,
        Check::Binding,
    ]
    .into_iter()
    .all(passed)
    {
        return None;
    }
    let mut basis = Basis::Clear;
    let mut behind = |is: bool| {
        if is {
            basis = Basis::EarlierSnapshot;
        }
    };
    let copy = match survey.outcome(Check::Wal) {
        Outcome::Passed => false,
        Outcome::Failed(Error::WalMissing) => true,
        _ => return None,
    };
    behind(copy);
    match survey.outcome(Check::Acceptance) {
        Outcome::Passed => {}
        Outcome::Failed(Error::WalMissing) => behind(true),
        _ => return None,
    }
    match survey.outcome(Check::Fence) {
        Outcome::Passed | Outcome::Failed(Error::Fenced) => {}
        Outcome::Failed(Error::CommitLost) => behind(true),
        _ => return None,
    }
    match survey.outcome(Check::Watermark) {
        Outcome::Passed => {}
        Outcome::Failed(Error::ReadUnavailable(_)) => behind(true),
        _ => return None,
    }
    Some(Plan {
        basis,
        identity: survey.identity()?.clone(),
        mark: survey.mark(),
        copy,
    })
}

fn read_open(binding: &Binding, plan: &Plan, deadline: Instant) -> Result<(bool, Vec<Item>)> {
    let connection = open_read_only(binding, deadline)?;
    read_rows(&connection, plan, deadline)
}

/// Identity and schema qualification, the watermark comparison and the rows. `true` when
/// the database is behind its watermark.
fn read_rows(connection: &Connection, plan: &Plan, deadline: Instant) -> Result<(bool, Vec<Item>)> {
    if connection::identity(connection, deadline)? != plan.identity {
        return Err(Error::Incompatible);
    }
    crate::ownership_schema::check(connection, &plan.identity.generation)?;
    let behind = match plan.mark.map(|mark| watermark::not_ahead(mark, connection)) {
        Some(Err(Error::CommitLost)) => true,
        Some(Err(error)) => return Err(error),
        Some(Ok(())) | None => false,
    };
    let items = maintenance_rows::read(connection, deadline)?;
    Ok((behind, items))
}
