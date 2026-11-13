//! Typed classification of why a read session refused, for the transport above.
//!
//! Variants follow the rows of the read-path wire mapping table; the transport owns
//! the wire codes, reasons and messages. Query-level errors (an invalid task query,
//! a stale cursor, an unavailable dataset) are not refusals and are not classified
//! here beyond the default row.
use crate::Error;
use rusqlite::ErrorCode;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadRefusal {
    /// Lease busy, deadline exceeded, or SQLite busy.
    Busy,
    /// `-wal` missing or not a regular file.
    WalMissing,
    /// An unreadable root, a lease that is invalid, or SQLite CANTOPEN or READONLY.
    Unavailable { file: &'static str },
    /// SQLite reported corruption (`malformed`, `not a database`).
    Corrupt { message: String },
    /// Marker, header or SQL identity does not match.
    InvalidMarker,
    /// The database application id is not this engine's (`foreign_store`).
    ForeignStore,
    /// The marker names an older schema (`unsupported_layout`).
    UnsupportedLayout,
    /// The marker or header names a newer schema than this engine.
    FutureSchema,
    /// A nonempty rollback journal.
    NonemptyJournal,
    /// The identity changed during the read; a retry is allowed.
    Replaced,
    /// A whole-store check failed and `store.fence` refuses the generation.
    Fenced,
    /// Any other error.
    Other { message: String },
}
impl From<&Error> for ReadRefusal {
    fn from(error: &Error) -> Self {
        match error {
            Error::WalMissing => Self::WalMissing,
            Error::Busy | Error::DeadlineExceeded => Self::Busy,
            Error::Sqlite(error) => from_sqlite(error),
            Error::ReadUnavailable(file) => Self::Unavailable { file },
            Error::ForeignStore => Self::ForeignStore,
            Error::UnsupportedLayout => Self::UnsupportedLayout,
            // An I/O failure the engine itself raised (no OS cause) is not a file fault.
            Error::Io(io)
                if io.kind() == std::io::ErrorKind::Other && io.raw_os_error().is_none() =>
            {
                Self::Other {
                    message: io.to_string(),
                }
            }
            // Not attributable to one file: the state root.
            Error::InvalidRoot | Error::Io(_) => Self::Unavailable { file: "state root" },
            Error::Incompatible => Self::InvalidMarker,
            Error::FutureSchema => Self::FutureSchema,
            Error::NeedsCheckpoint => Self::NonemptyJournal,
            Error::Replaced => Self::Replaced,
            Error::Fenced => Self::Fenced,
            other => Self::Other {
                message: other.to_string(),
            },
        }
    }
}
fn from_sqlite(error: &rusqlite::Error) -> ReadRefusal {
    match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => ReadRefusal::Busy,
        Some(ErrorCode::CannotOpen | ErrorCode::ReadOnly) => ReadRefusal::Unavailable {
            file: "store.sqlite3",
        },
        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => ReadRefusal::Corrupt {
            message: error.to_string(),
        },
        _ => ReadRefusal::Other {
            message: error.to_string(),
        },
    }
}
