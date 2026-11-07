//! Root lifecycle: creation, removal, state, and the guard that keeps `-wal` alive.
//!
//! The store connection suppresses checkpoints, so a clean close leaves `-wal` in place; the
//! guard proves it. A missing `-wal` is checked before an open, because SQLite would
//! silently recreate an empty one and the history would be lost.
use crate::fail::{Fail, Result};
use hf_application::lease::LeasePort;
use hf_domain::task_read::Coverage;
use hf_interfaces::fixture_port::Response;
use hf_storage::{Limits, Store, TaskFixture};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
const WAL: &str = "store.sqlite3-wal";
pub struct Context {
    pub root: PathBuf,
    leases: Arc<dyn LeasePort>,
}
impl Context {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            leases: hf_platform::lease::shared(),
        }
    }
    pub fn deadline(&self) -> Instant {
        Instant::now() + Duration::from_secs(30)
    }
    pub fn limits() -> Limits {
        Limits {
            max_rows: 4096,
            max_bytes: 16 * 1024 * 1024,
        }
    }
    pub fn leases(&self) -> Arc<dyn LeasePort> {
        self.leases.clone()
    }
    /// Opens the existing store for one mutating operation and proves `-wal` survived.
    pub fn with_store<T>(&self, action: impl FnOnce(&Store) -> Result<T>) -> Result<T> {
        require_wal(&self.root)?;
        let store = Store::open(&self.root, Self::limits(), self.deadline(), self.leases())?;
        let outcome = action(&store);
        drop(store);
        finish(outcome, require_wal(&self.root))
    }
}
/// The outcome of the operation, unless it succeeded and `-wal` is gone.
pub fn finish<T>(outcome: Result<T>, wal: Result<()>) -> Result<T> {
    match (outcome, wal) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
    }
}
pub fn require_wal(root: &Path) -> Result<()> {
    match std::fs::symlink_metadata(root.join(WAL)) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        _ => Err(Fail::new(
            "store.sqlite3-wal is missing or not a regular file",
        )),
    }
}
/// Creates the root and an empty, fully covered task dataset.
pub fn initialize(context: &Context) -> Result<Store> {
    let store = Store::initialize_fixture(
        &context.root,
        Context::limits(),
        context.deadline(),
        context.leases(),
    )?;
    let all = Coverage {
        discovery_complete: true,
        ownership_complete: true,
        source_complete: true,
    };
    let empty = TaskFixture {
        rows: Vec::new(),
        pending: Vec::new(),
        coverage: all,
    };
    store.replace_task_fixture(&empty, context.deadline())?;
    Ok(store)
}
pub fn initialize_empty(context: &Context) -> Result<Response> {
    let store = initialize(context)?;
    drop(store);
    require_wal(&context.root)?;
    Ok(Response::Null)
}
pub fn remove_root(context: &Context) -> Result<Response> {
    match std::fs::remove_dir_all(&context.root) {
        Ok(()) => Ok(Response::Null),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Response::Null),
        Err(error) => Err(error.into()),
    }
}
pub fn make_foreign_root(context: &Context) -> Result<Response> {
    std::fs::create_dir(&context.root)?;
    std::fs::write(context.root.join("stranger.txt"), "not harflow")?;
    Ok(Response::Null)
}
/// `missing`, `initialized` (a current marker and a regular `store.lock`) or `other`.
pub fn root_state(context: &Context) -> &'static str {
    let root = &context.root;
    let metadata = match std::fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return "missing",
        Err(_) => return "other",
    };
    let lock = std::fs::symlink_metadata(root.join("store.lock"));
    if metadata.is_dir() && hf_storage::marker_is_current(root) && lock.is_ok_and(|m| m.is_file()) {
        "initialized"
    } else {
        "other"
    }
}
