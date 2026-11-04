#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! Internal fixture storage only. No product state or public engine API.
//!
//! Paths must be cooperatively managed. Metadata checks reject observed replacement
//! and symlinks; they do not provide atomic nofollow protection against hostile races.
mod binding;
mod connection;
mod operations;
#[cfg(test)]
mod tests;
mod wal;
use binding::Binding;
use rusqlite::Connection;
use std::{path::Path, sync::Mutex, time::Instant};

pub const SCHEMA_VERSION: u32 = 1;
pub const APPLICATION_ID: u32 = 1_212_560_210;
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    InvalidRoot,
    Replaced,
    Incompatible,
    FutureSchema,
    Busy,
    DeadlineExceeded,
    LimitExceeded,
    CommitOutcomeUnknown,
    NeedsCheckpoint,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub generation: String,
    pub schema_version: u32,
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_rows: usize,
    pub max_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_rows: 128,
            max_bytes: 1_048_576,
        }
    }
}
#[derive(Debug, Clone)]
pub enum Mutation {
    Insert { key: String, value: Vec<u8> },
    Put { key: String, value: Vec<u8> },
    Delete { key: String },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub key: String,
    pub value: Vec<u8>,
}
pub struct Store {
    connection: Mutex<Connection>,
    binding: Binding,
    identity: Identity,
    limits: Limits,
    _claim: std::fs::File,
}
impl Store {
    pub fn initialize_fixture(
        root: impl AsRef<Path>,
        limits: Limits,
        deadline: Instant,
    ) -> Result<Self> {
        connection::check_deadline(deadline)?;
        Binding::validate_parent(root.as_ref())?;
        std::fs::create_dir(root.as_ref()).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Error::InvalidRoot
            } else {
                error.into()
            }
        })?;
        let claim = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(root.as_ref().join("store.lock"))?;
        connection::claim(&claim, false)?;
        let binding = Binding::capture(root.as_ref(), false)?;
        let mut connection = Connection::open(binding.database())?;
        binding.check_root()?;
        connection::configure(&connection, deadline)?;
        connection::initialize(&mut connection, deadline)?;
        let binding = Binding::capture(root.as_ref(), true)?;
        let identity = connection::identity(&connection, deadline)?;
        Ok(Self {
            connection: Mutex::new(connection),
            binding,
            identity,
            limits,
            _claim: claim,
        })
    }
    pub fn open(root: impl AsRef<Path>, limits: Limits, deadline: Instant) -> Result<Self> {
        connection::check_deadline(deadline)?;
        let binding = Binding::capture(root.as_ref(), true)?;
        let claim = std::fs::File::open(root.as_ref().join("store.lock"))?;
        connection::claim(&claim, false)?;
        binding.check()?;
        connection::qualify_header(&binding.database())?;
        wal::qualify(&binding.database())?;
        let identity = connection::read_identity(&binding.database(), deadline)?;
        binding.check()?;
        let connection = Connection::open_with_flags(
            binding.database(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?;
        binding.check()?;
        let observed = connection::identity(&connection, deadline)?;
        if observed != identity {
            return Err(Error::Replaced);
        }
        connection::configure(&connection, deadline)?;
        binding.check()?;
        Ok(Self {
            connection: Mutex::new(connection),
            binding,
            identity,
            limits,
            _claim: claim,
        })
    }
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
}
/// Missing roots create nothing. Qualification reads main-file identity/version first.
/// Nonempty WAL refuses with NeedsCheckpoint before SQLite is opened. Initial
/// open/probe require cooperative quiescence; detached multiprocess readiness is
/// not qualified. Already open handles support normal SQLite WAL transactions.
pub fn probe(root: impl AsRef<Path>) -> Result<Option<Identity>> {
    let deadline = Instant::now() + std::time::Duration::from_secs(1);
    Binding::validate_probe_path(root.as_ref())?;
    if !root.as_ref().try_exists()? {
        return Ok(None);
    }
    let binding = Binding::capture(root.as_ref(), true)?;
    let claim = std::fs::File::open(root.as_ref().join("store.lock"))?;
    connection::claim(&claim, true)?;
    binding.check()?;
    connection::qualify_header(&binding.database())?;
    wal::qualify(&binding.database())?;
    let identity = connection::read_identity(&binding.database(), deadline)?;
    binding.check()?;
    Ok(Some(identity))
}
