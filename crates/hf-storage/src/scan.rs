//! One bounded in-place scan over a read-only connection.
//!
//! A watchdog thread interrupts the statement when the deadline passes, so a scan
//! over a store of any size ends in `DeadlineExceeded` rather than running on. The
//! watchdog is joined before the scan returns, including on an early finish. The
//! caller drops the connection afterwards and never reuses it: an interrupt that
//! lands just as the statement ends must not reach a later operation.
use crate::{Error, Result, connection};
use rusqlite::{Connection, ErrorCode};
use std::{
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant},
};

/// Interval at which a lapsed deadline re-issues the interrupt.
const REINTERRUPT: Duration = Duration::from_millis(20);

/// Runs `run` on `connection`, interrupting it when `deadline` passes. A value that
/// arrives after the deadline is refused as `DeadlineExceeded`.
pub(crate) fn bounded<T>(
    connection: &Connection,
    deadline: Instant,
    run: impl FnOnce(&Connection) -> rusqlite::Result<T>,
) -> Result<T> {
    let value = bounded_late(connection, deadline, run)?;
    connection::check_deadline(deadline)?;
    Ok(value)
}

/// As `bounded`, but a value that completed after the deadline is returned: the caller
/// decides whether a late answer still counts.
pub(crate) fn bounded_late<T>(
    connection: &Connection,
    deadline: Instant,
    run: impl FnOnce(&Connection) -> rusqlite::Result<T>,
) -> Result<T> {
    connection::timeout(connection, deadline)?;
    let handle = connection.get_interrupt_handle();
    let (finished, watch) = mpsc::channel::<()>();
    let result = std::thread::scope(|scope| {
        scope.spawn(move || {
            // SQLite clears the interrupt flag when a statement starts, so one early
            // interrupt can be lost. Keep interrupting until the scan signals completion.
            let mut wait = deadline.saturating_duration_since(Instant::now());
            while matches!(watch.recv_timeout(wait), Err(RecvTimeoutError::Timeout)) {
                handle.interrupt();
                wait = REINTERRUPT;
            }
        });
        let result = run(connection);
        // Dropping the sender wakes the watchdog; the scope joins it.
        drop(finished);
        result
    });
    result.map_err(|error| classify(error, deadline))
}

/// `PRAGMA quick_check(1)`: anything but `ok` is `Incompatible`.
pub(crate) fn quick_check(connection: &Connection, deadline: Instant) -> Result<()> {
    let verdict: String = bounded(connection, deadline, |db| {
        db.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
    })?;
    if verdict == "ok" {
        Ok(())
    } else {
        Err(Error::Incompatible)
    }
}

/// Corruption found by any in-place read is `Incompatible`, as in the scan.
pub(crate) fn corruption(error: Error) -> Error {
    match error {
        Error::Sqlite(rusqlite::Error::SqliteFailure(failure, _))
            if matches!(
                failure.code,
                ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase
            ) =>
        {
            Error::Incompatible
        }
        other => other,
    }
}

fn classify(error: rusqlite::Error, deadline: Instant) -> Error {
    let code = match &error {
        rusqlite::Error::SqliteFailure(failure, _) => Some(failure.code),
        _ => None,
    };
    match code {
        Some(ErrorCode::OperationInterrupted) => Error::DeadlineExceeded,
        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => Error::Incompatible,
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => {
            if Instant::now() >= deadline {
                Error::DeadlineExceeded
            } else {
                Error::Busy
            }
        }
        _ => Error::Sqlite(error),
    }
}
