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
mod event_writer;
pub use event_fixture::EventFixture;
pub use event_writer::{EVENT_BODY_LIMIT, EVENT_KIND_LIMIT, EVENT_LINE_LIMIT, EventAppend};
mod checkpoint;
mod connection;
pub use checkpoint::Checkpoint;
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
mod write_tx;
pub use task_fixture::TaskFixture;
pub use task_read::TaskReadSession;
mod qualify;
mod recovery_copy;
mod scan;
#[cfg(test)]
mod task_query_tests;
#[cfg(test)]
mod tests;
mod wal;
mod wal_acceptance;
mod wal_owner;
pub use wal_owner::{
    CheckpointPolicy, JOURNAL_SIZE_LIMIT, MAINTENANCE_BUDGET, PASSIVE_AFTER, SyncAllFlush,
    WalMaintenance, WriterOptions,
};
mod writer_open;
use binding::Binding;
use hf_application::lease::{LeaseError, LeaseGuard, LeaseMode, LeasePort};
use rusqlite::Connection;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};
pub use writer_open::{probe, probe_with_deadline};

pub const SCHEMA_VERSION: u32 = 7;
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
    /// The task writer: a task with this id already exists, valid or damaged.
    TaskExists,
    /// The task writer: no valid task with this id exists.
    TaskMissing,
    /// The event writer: an event's seq is not above the last valid record's.
    EventOutOfOrder,
    /// The event writer: the stream stores an EOF seq, so its task finished and appends are refused.
    EventStreamClosed,
    /// The artifact writer: the task's recorded status is terminal, so a caller can no longer
    /// register a reference.
    TaskFinished,
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
/// The default per-read byte budget of `Limits`.
pub const DEFAULT_MAX_BYTES: usize = 1_048_576;
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_rows: usize,
    pub max_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_rows: 128,
            max_bytes: DEFAULT_MAX_BYTES,
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
    options: WriterOptions,
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
        Self::open_with(root, limits, deadline, leases, WriterOptions::suppressed())
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
mod task_watch;
mod watch_meta;
pub use task_watch::WatchSession;
mod watch_control;
pub use watch_control::RealWatchControl;

mod chain_stops;
mod launch_attempts;
mod pending_reads;
pub use chain_stops::ChainStop;
pub use launch_attempts::LaunchRow;

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
pub use fixture_events::FixtureLine;
pub use fixture_extras::{FixtureArtifact, FixturePlan};
pub use fixture_task::{FixtureFacts, FixtureProcess, FixturePrompt, FixtureTask};
mod artifact_writer;
mod task_republish;
mod task_writer;
pub use artifact_writer::{
    ARTIFACT_ITEMS_LIMIT, ARTIFACT_LABELS_LIMIT, ARTIFACT_TEXT_LIMIT, MANIFEST_BYTES_LIMIT,
    ManifestLengths, Registered, Registrant,
};
pub use task_republish::Republished;
pub use task_writer::{
    FACT_TEXT_LIMIT, PROMPT_BLOB_LIMIT, PROMPT_PREVIEW_CHARS, REPO_PATH_LIMIT, TITLE_LIMIT,
    TaskCreate, TaskFacts, TaskProcess,
};

/// Whether `root` holds a readable store marker for this schema. Reads one file and
/// opens nothing else; a missing, unreadable or foreign marker is `false`.
pub fn marker_is_current(root: impl AsRef<Path>) -> bool {
    let marker = root.as_ref().join("store.identity");
    std::fs::symlink_metadata(&marker).is_ok_and(|m| m.is_file())
        && wal::read_marker_at(root.as_ref(), true).is_ok()
}
