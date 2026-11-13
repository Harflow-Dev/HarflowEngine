//! Read-only access to the real store under a shared lease.
//!
//! No `io.lock`, no private copy, no per-call `quick_check`, no size limit and no
//! checkpoint or write path. A missing `-wal` is refused before SQLite is opened,
//! because opening would create an empty one and report an empty history. The
//! deadline is soft: SQLite open and WAL recovery cannot be interrupted, so an
//! overrun answer is refused after the fact as `DeadlineExceeded`.
use crate::{
    Error, Identity, Limits, Result, TaskSnapshot, binding::Binding, connection,
    read_context::ReadContext, wal, wal_acceptance,
};
use hf_application::{
    event_read::EventReadResult,
    lease::{LeaseGuard, LeaseMode, LeasePort},
    task_read::TaskReadResult,
};
use hf_domain::{
    event_read::EventQuery,
    task_read::{AuthorizedScope, TaskQuery},
};
use rusqlite::{Connection, OpenFlags};
use std::{
    path::Path,
    sync::{Arc, Mutex, TryLockError},
    time::{Duration, Instant},
};
/// Cap on shared-lease acquisition.
pub const LEASE_BUDGET: Duration = Duration::from_secs(2);
/// Default deadline for one read, for callers that have none of their own.
pub const READ_BUDGET: Duration = Duration::from_secs(5);
pub struct ReadSession {
    connection: Mutex<Connection>,
    binding: Binding,
    identity: Identity,
    limits: Limits,
    _claim: Box<dyn LeaseGuard>,
}
impl ReadSession {
    pub fn open(
        root: impl AsRef<Path>,
        limits: Limits,
        deadline: Instant,
        leases: Arc<dyn LeasePort>,
    ) -> Result<Self> {
        connection::check_deadline(deadline)?;
        let root = std::path::absolute(root.as_ref())?;
        let binding = match Binding::identify_existing(&root, leases) {
            Err(error @ (Error::InvalidRoot | Error::Io(_))) => {
                let fault = marker_file_fault(&root)
                    .or_else(|| non_regular(&root, &["store.lock", "io.lock"]));
                return Err(fault.unwrap_or(error));
            }
            other => other?,
        };
        let lease_deadline = deadline.min(Instant::now() + LEASE_BUDGET);
        let claim = binding
            .acquire(LeaseMode::Shared, lease_deadline)
            .map_err(named("store.lock"))?;
        // Spec order: marker, header, rollback journal, `-wal`, acceptance record. The `-wal` type is
        // left out of the sidecar check so that a defect there cannot mask the others.
        binding.check_sidecars(false).map_err(|error| {
            non_regular(&root, &["store.sqlite3-shm", "store.sqlite3-journal"]).unwrap_or(error)
        })?;
        let expected = wal::marker_distinct(&binding).map_err(named("store.identity"))?;
        connection::qualify_header_distinct(&binding.database()).map_err(named("store.sqlite3"))?;
        wal::reject_nonempty_journal(&binding).map_err(named("store.sqlite3-journal"))?;
        require_wal(&binding)?;
        acceptance(&binding, &expected)?;
        let connection = open_read_only(&binding, deadline)?;
        if connection::identity(&connection, deadline)? != expected {
            return Err(Error::Incompatible);
        }
        check_files(&binding, &expected)?;
        Ok(Self {
            connection: Mutex::new(connection),
            binding,
            identity: expected,
            limits,
            _claim: claim,
        })
    }
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    pub fn task_read(
        &self,
        scope: &AuthorizedScope,
        query: &TaskQuery,
        deadline: Instant,
    ) -> Result<TaskReadResult> {
        query.validate().map_err(|_| Error::InvalidTaskQuery)?;
        scope.validate().map_err(|_| Error::InvalidTaskQuery)?;
        self.read(deadline, |context, db| {
            crate::task_read::run(context, db, scope, query, deadline)
        })
    }
    pub fn event_read(&self, query: &EventQuery, deadline: Instant) -> Result<EventReadResult> {
        query.validate().map_err(|_| Error::InvalidEventQuery)?;
        self.read(deadline, |context, db| {
            crate::event_read::run(context, db, query, deadline)
        })
    }
    /// Runs `action` inside one read transaction over the typed task queries.
    pub fn snapshot<T>(
        &self,
        deadline: Instant,
        action: impl FnOnce(&TaskSnapshot<'_>) -> Result<T>,
    ) -> Result<T> {
        self.read(deadline, |context, db| {
            action(&TaskSnapshot::new(context, db, deadline))
        })
    }
    /// One read transaction: binding, marker and SQL identity are checked on entry and
    /// the binding and deadline on exit.
    fn read<T>(
        &self,
        deadline: Instant,
        action: impl FnOnce(&ReadContext<'_>, &Connection) -> Result<T>,
    ) -> Result<T> {
        connection::check_deadline(deadline)?;
        let mut guard = self.connection.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => Error::Busy,
            TryLockError::Poisoned(_) => {
                Error::Io(std::io::Error::other("read connection poisoned"))
            }
        })?;
        self.binding.check_sidecars(false)?;
        if wal::marker_distinct(&self.binding).map_err(named("store.identity"))? != self.identity {
            return Err(Error::Replaced);
        }
        require_wal(&self.binding)?;
        acceptance(&self.binding, &self.identity)?;
        connection::timeout(&guard, deadline)?;
        let transaction = guard.transaction()?;
        if connection::identity(&transaction, deadline)? != self.identity {
            return Err(Error::Replaced);
        }
        let context = ReadContext {
            identity: &self.identity,
            limits: self.limits,
        };
        let value = action(&context, &transaction)?;
        check_files(&self.binding, &self.identity)?;
        connection::check_deadline(deadline)?;
        Ok(value)
    }
}
pub(crate) fn open_read_only(binding: &Binding, deadline: Instant) -> Result<Connection> {
    connection::check_deadline(deadline)?;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(binding.database(), flags)?;
    connection::suppress_checkpoints(&connection).map_err(|error| match error {
        Error::Incompatible => Error::Io(std::io::Error::other("checkpoint suppression failed")),
        other => other,
    })?;
    Ok(connection)
}
fn wal_path(root: &Path) -> std::path::PathBuf {
    root.join("store.sqlite3-wal")
}
/// Names the file when an I/O or root failure came from a step that touches one.
fn named(file: &'static str) -> impl FnOnce(Error) -> Error {
    move |error| match error {
        Error::InvalidRoot => Error::ReadUnavailable(file),
        Error::Io(io) if io.raw_os_error().is_some() || io.kind() != std::io::ErrorKind::Other => {
            Error::ReadUnavailable(file)
        }
        other => other,
    }
}
/// Names the first of `names` that exists in `root` and is not a regular file.
fn non_regular(root: &Path, names: &[&'static str]) -> Option<Error> {
    names.iter().find_map(|name| {
        let metadata = std::fs::symlink_metadata(root.join(name)).ok()?;
        (!metadata.is_file()).then_some(Error::ReadUnavailable(name))
    })
}
/// A marker or database that is not a regular file, or a missing database, reads as
/// an invalid marker, as the lease-free inspector reports it.
fn marker_file_fault(root: &Path) -> Option<Error> {
    if !std::fs::symlink_metadata(root).is_ok_and(|metadata| metadata.is_dir()) {
        return None;
    }
    for name in ["store.identity", "store.sqlite3"] {
        match std::fs::symlink_metadata(root.join(name)) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => return Some(Error::Incompatible),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Some(Error::Incompatible);
            }
            Err(_) => return None,
        }
    }
    None
}
/// Binding and sidecar types, with a defective `-wal` reported as a missing one,
/// then the acceptance record for the session's generation.
fn check_files(binding: &Binding, identity: &Identity) -> Result<()> {
    binding.check_sidecars(false)?;
    require_wal(binding)?;
    acceptance(binding, identity)
}

/// The lost-WAL acceptance record, checked right after `-wal`.
fn acceptance(binding: &Binding, identity: &Identity) -> Result<()> {
    wal_acceptance::check(binding.root(), &identity.generation)
        .map_err(named("store.wal-acceptance"))
}
/// `lstat` only: a missing or non-regular `-wal` is not a legitimate engine state.
/// SQLite would create an empty one on open (`pagerOpenWalIfPresent` finds none)
/// and report an empty history.
fn require_wal(binding: &Binding) -> Result<()> {
    match std::fs::symlink_metadata(wal_path(binding.root())) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(Error::WalMissing),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(Error::WalMissing),
        Err(error) => Err(Error::from(error)).map_err(named("store.sqlite3-wal")),
    }
}
