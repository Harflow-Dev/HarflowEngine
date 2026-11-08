#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! Internal fixture storage only. No product state or public engine API.
//!
//! Paths must be cooperatively managed. Metadata checks reject observed replacement
//! and symlinks; they do not provide atomic nofollow protection against hostile races.
mod binding;
mod event_checks;
mod event_fixture;
mod event_read;
mod event_render;
mod event_scan;
pub use event_fixture::EventFixture;
mod checkpoint;
mod connection;
pub use checkpoint::Checkpoint;
use connection::open_source;
mod operations;
mod ownership;
mod ownership_codec;
mod ownership_port;
mod ownership_read;
mod ownership_release;
mod ownership_schema;
mod ownership_write;
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
use hf_application::lease::{LeaseError, LeaseGuard, LeaseMode, LeasePort};
use rusqlite::Connection;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};

pub const SCHEMA_VERSION: u32 = 6;
pub const APPLICATION_ID: u32 = 1_212_560_210;
#[derive(Debug)]
pub enum Error {
    Conversation(hf_domain::conversation::Error),
    TaskWatchUnavailable,
    InvalidOwnership,
    OwnershipConflict,
    StaleOwnership,
    OwnershipUnavailable,
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
    WalMissing,
    /// Raised only by the read session: the database application id is not ours.
    ForeignStore,
    /// Raised only by the read session: the marker names an older schema.
    UnsupportedLayout,
    /// Raised only by the read session: the named file could not be used.
    ReadUnavailable(&'static str),
    /// Raised only by the ctl task reads: a stored row does not decode.
    MalformedRow,
    TaskReadUnavailable,
    InvalidTaskQuery,
    StaleTaskCursor,
    EventReadUnavailable,
    InvalidEventQuery,
    ExpiredEventCursor,
    /// Fixture writers only: the task already exists.
    FixtureConflict,
    /// Fixture writers only: the task or its log does not exist, or an append
    /// targets a damaged task, whose stream facts have no source.
    FixtureMissing,
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
    _claim: Box<dyn LeaseGuard>,
}
impl Store {
    pub fn initialize_fixture(
        root: impl AsRef<Path>,
        limits: Limits,
        deadline: Instant,
        leases: Arc<dyn LeasePort>,
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
        let claim = leases
            .acquire(
                &root.join("store.lock"),
                LeaseMode::Exclusive,
                true,
                deadline,
            )
            .map_err(lease_error)?;
        let io = leases
            .acquire(&root.join("io.lock"), LeaseMode::Exclusive, true, deadline)
            .map_err(lease_error)?;
        let binding = Binding::capture(root, false, leases.clone())?;
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
        Self::open(root, limits, deadline, leases)
    }
    pub fn open(
        root: impl AsRef<Path>,
        limits: Limits,
        deadline: Instant,
        leases: Arc<dyn LeasePort>,
    ) -> Result<Self> {
        let root = std::path::absolute(root.as_ref())?;
        let parent = root.parent().ok_or(Error::InvalidRoot)?;
        Self::open_with_scratch(&root, limits, deadline, parent, leases)
    }
    /// Internal permission to rebuild WAL-index/recover valid crash WAL after isolated qualification.
    pub fn open_with_scratch(
        root: impl AsRef<Path>,
        limits: Limits,
        deadline: Instant,
        scratch_parent: impl AsRef<Path>,
        leases: Arc<dyn LeasePort>,
    ) -> Result<Self> {
        connection::check_deadline(deadline)?;
        let binding = Binding::capture(root.as_ref(), true, leases)?;
        let claim = binding.claim(LeaseMode::Shared, deadline)?;
        let _io = binding.io(deadline)?;
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
fn lease_error(error: LeaseError) -> Error {
    match error {
        LeaseError::Busy => Error::Busy,
        LeaseError::DeadlineExceeded => Error::DeadlineExceeded,
        LeaseError::LimitExceeded => Error::LimitExceeded,
        LeaseError::Invalid => Error::InvalidRoot,
        LeaseError::Replaced => Error::Replaced,
        LeaseError::Unavailable => Error::Io(std::io::Error::other("lease unavailable")),
    }
}
/// Missing roots create nothing; existing roots are qualified using only a private copy.
pub fn probe(root: impl AsRef<Path>, leases: Arc<dyn LeasePort>) -> Result<Option<Identity>> {
    let root = std::path::absolute(root.as_ref())?;
    probe_with_scratch(
        &root,
        Instant::now() + std::time::Duration::from_secs(1),
        root.parent().ok_or(Error::InvalidRoot)?,
        leases,
    )
}
pub fn probe_with_scratch(
    root: impl AsRef<Path>,
    deadline: Instant,
    scratch_parent: impl AsRef<Path>,
    leases: Arc<dyn LeasePort>,
) -> Result<Option<Identity>> {
    connection::check_deadline(deadline)?;
    Binding::validate_probe_path(root.as_ref())?;
    if !root.as_ref().try_exists()? {
        return Ok(None);
    }
    let binding = Binding::capture(root.as_ref(), true, leases)?;
    let _claim = binding.claim(LeaseMode::Shared, deadline)?;
    let _io = binding.io(deadline)?;
    let identity = wal::qualify(&binding, scratch_parent.as_ref(), deadline)?;
    binding.check()?;
    Ok(Some(identity))
}

mod task_watch;
mod watch_meta;
pub use task_watch::WatchSession;
mod watch_control;
pub use watch_control::RealWatchControl;

mod pending_fixture;

mod capture;
mod capture_codec;
mod capture_read;
mod capture_schema;

mod read_context;
mod read_refusal;
mod read_session;
pub use read_refusal::ReadRefusal;
pub use read_session::{LEASE_BUDGET, READ_BUDGET, ReadSession};
mod caller_scan;
mod feed_reads;
mod history_reads;
mod recent_reads;
mod snapshot;
pub mod statements;
mod task_catalog;
pub use snapshot::TaskSnapshot;
mod recovery;
mod recovery_events;
pub use recovery::RecoveryReadSession;

mod owner_port;

mod root_hold;
pub use root_hold::RootHold;
mod participant;
pub use participant::ParticipantIdentity;
mod digest;
pub use digest::{DigestSizeLimit, Sha256Digest, Sha256Stream};

mod conversation;
pub use conversation::FreshGrant;
mod conversation_codec;
mod conversation_data;

mod fixture_events;
mod fixture_extras;
mod fixture_schema;
mod fixture_task;
mod fixture_tx;
pub use fixture_events::FixtureLine;
pub use fixture_extras::FixtureArtifact;
pub use fixture_task::{FixtureFacts, FixtureProcess, FixturePrompt, FixtureTask};

/// Whether `root` holds a readable store marker for this schema. Reads one file and
/// opens nothing else; a missing, unreadable or foreign marker is `false`.
pub fn marker_is_current(root: impl AsRef<Path>) -> bool {
    let marker = root.as_ref().join("store.identity");
    std::fs::symlink_metadata(&marker).is_ok_and(|m| m.is_file())
        && wal::read_marker_at(root.as_ref(), true).is_ok()
}
