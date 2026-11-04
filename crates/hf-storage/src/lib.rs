#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! Internal fixture storage only. No product state or public engine API.
//!
//! Paths must be cooperatively managed. Metadata checks reject observed replacement
//! and symlinks; they do not provide atomic nofollow protection against hostile races.
mod binding;
mod checkpoint;
mod connection;
pub use checkpoint::Checkpoint;
mod operations;
mod task_fixture;
mod task_page;
mod task_query;
mod task_read;
pub use task_fixture::TaskFixture;
pub use task_read::TaskReadSession;
#[cfg(test)]
mod task_query_tests;
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
    TaskReadUnavailable,
    InvalidTaskQuery,
    StaleTaskCursor,
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
        let root = root.as_ref();
        Binding::validate_parent(root)?;
        std::fs::create_dir(root).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Error::InvalidRoot
            } else {
                error.into()
            }
        })?;
        let claim = create_lock(root, "store.lock")?;
        connection::claim(&claim, false)?;
        let io = create_lock(root, "io.lock")?;
        connection::claim(&io, false)?;
        let binding = Binding::capture(root, false)?;
        let mut connection = Connection::open(binding.database())?;
        connection::suppress_checkpoints(&connection)?;
        binding.check_root()?;
        connection::configure(&connection, deadline)?;
        connection::initialize(&mut connection, deadline)?;
        let identity = connection::identity(&connection, deadline)?;
        connection::check_deadline(deadline)?;
        wal::publish(root, &identity)?;
        drop(connection);
        drop(io);
        drop(claim);
        Self::open(root, limits, deadline)
    }
    pub fn open(root: impl AsRef<Path>, limits: Limits, deadline: Instant) -> Result<Self> {
        let root = std::path::absolute(root.as_ref())?;
        let parent = root.parent().ok_or(Error::InvalidRoot)?;
        Self::open_with_scratch(&root, limits, deadline, parent)
    }
    /// Internal permission to rebuild WAL-index/recover valid crash WAL after isolated qualification.
    pub fn open_with_scratch(
        root: impl AsRef<Path>,
        limits: Limits,
        deadline: Instant,
        scratch_parent: impl AsRef<Path>,
    ) -> Result<Self> {
        connection::check_deadline(deadline)?;
        let binding = Binding::capture(root.as_ref(), true)?;
        let claim = std::fs::File::open(root.as_ref().join("store.lock"))?;
        connection::claim(&claim, true)?;
        let _io = binding.io()?;
        let identity = wal::qualify(&binding, scratch_parent.as_ref(), deadline)?;
        let connection = open_source(&binding, &identity, deadline)?;
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
fn create_lock(root: &Path, name: &str) -> Result<std::fs::File> {
    Ok(std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(root.join(name))?)
}
fn open_source(binding: &Binding, identity: &Identity, deadline: Instant) -> Result<Connection> {
    binding.check()?;
    #[cfg(test)]
    crate::tests::stage("before_source_open");
    connection::check_deadline(deadline)?;
    let connection = Connection::open_with_flags(
        binding.database(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    )?;
    connection::suppress_checkpoints(&connection)?;
    #[cfg(test)]
    crate::tests::stage("after_source_open");
    if connection::identity(&connection, deadline)? != *identity {
        return Err(Error::Replaced);
    }
    connection::configure(&connection, deadline)?;
    binding.check()?;
    Ok(connection)
}
/// Missing roots create nothing; existing roots are qualified using only a private copy.
pub fn probe(root: impl AsRef<Path>) -> Result<Option<Identity>> {
    let root = std::path::absolute(root.as_ref())?;
    probe_with_scratch(
        &root,
        Instant::now() + std::time::Duration::from_secs(1),
        root.parent().ok_or(Error::InvalidRoot)?,
    )
}
pub fn probe_with_scratch(
    root: impl AsRef<Path>,
    deadline: Instant,
    scratch_parent: impl AsRef<Path>,
) -> Result<Option<Identity>> {
    connection::check_deadline(deadline)?;
    Binding::validate_probe_path(root.as_ref())?;
    if !root.as_ref().try_exists()? {
        return Ok(None);
    }
    let binding = Binding::capture(root.as_ref(), true)?;
    let claim = std::fs::File::open(root.as_ref().join("store.lock"))?;
    connection::claim(&claim, true)?;
    let _io = binding.io()?;
    let identity = wal::qualify(&binding, scratch_parent.as_ref(), deadline)?;
    binding.check()?;
    Ok(Some(identity))
}
