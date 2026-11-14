#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Shared helpers for the state-recovery gate tests. Record bytes are written here
//! directly, independently of the storage and domain code that reads them.

use crate::common::{self, default_limits};
use crate::read_session_support::{copy_tree, later, names};
use hf_application::{
    lease::LeasePort,
    recovery::{RecoveryError, RecoveryStorePort},
};
use hf_domain::ownership::Id;
use hf_storage::{Error, ReadSession, RecoveryReadSession, RootHold, Store, probe_with_deadline};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

pub const RECORD: &str = "store.recovery";
pub const OP: &str = "0123456789abcdef0123456789abcdef";
pub const OTHER_OP: &str = "fedcba9876543210fedcba9876543210";
pub const SHM: &str = "store.sqlite3-shm";

pub fn record_bytes(operation: &str, state: &str) -> Vec<u8> {
    format!("harflow-recovery-1\n{operation}\n{state}\n").into_bytes()
}

pub fn write_record(root: &Path, state: &str) {
    fs::write(root.join(RECORD), record_bytes(OP, state)).unwrap();
}

/// What one opener answered: a storage result, or the recovery read port's result.
#[derive(Debug)]
pub enum Outcome {
    Opened,
    Store(Error),
    Recovery(RecoveryError),
}

impl Outcome {
    fn of<T>(result: hf_storage::Result<T>) -> Self {
        result.map_or_else(Self::Store, |_| Self::Opened)
    }
    /// The gate's refusal: `RecoveryInterrupted`, which the recovery read port maps to
    /// `Unavailable` (as it maps an I/O failure; the other openers tell them apart).
    pub fn is_interrupted(&self) -> bool {
        matches!(
            self,
            Self::Store(Error::RecoveryInterrupted) | Self::Recovery(RecoveryError::Unavailable)
        )
    }
    /// A comparable label: the error variant, with the I/O error kind.
    pub fn kind(&self) -> String {
        match self {
            Self::Opened => "opened".to_string(),
            Self::Store(Error::Io(error)) => format!("Io({:?})", error.kind()),
            Self::Store(error) => format!("{error:?}"),
            Self::Recovery(error) => format!("recovery {error:?}"),
        }
    }
}

pub type Opener = fn(&Path, Arc<dyn LeasePort>) -> Outcome;

fn open_entry(root: &Path, leases: Arc<dyn LeasePort>) -> Outcome {
    Outcome::of(Store::open(root, default_limits(), later(10), leases))
}

fn recover_entry(root: &Path, leases: Arc<dyn LeasePort>) -> Outcome {
    Outcome::of(Store::recover(root, later(10), leases))
}

fn probe_entry(root: &Path, leases: Arc<dyn LeasePort>) -> Outcome {
    Outcome::of(probe_with_deadline(root, later(10), leases))
}

fn root_hold_entry(root: &Path, leases: Arc<dyn LeasePort>) -> Outcome {
    Outcome::of(RootHold::open(root, later(10), leases))
}

fn recovery_read_entry(root: &Path, leases: Arc<dyn LeasePort>) -> Outcome {
    let scratch = root
        .parent()
        .map_or_else(|| PathBuf::from("/"), Path::to_path_buf)
        .join("recovery-scratch");
    fs::create_dir_all(&scratch).unwrap();
    let session = RecoveryReadSession::new(root, scratch, default_limits(), leases);
    let operation = Id::new("1".repeat(32)).unwrap();
    match session.read(&operation, later(10)) {
        Ok(_) => Outcome::Opened,
        Err(error) => Outcome::Recovery(error),
    }
}

fn read_session_entry(root: &Path, leases: Arc<dyn LeasePort>) -> Outcome {
    Outcome::of(ReadSession::open(root, default_limits(), later(10), leases))
}

/// Every store opener the gate covers.
pub const OPENERS: [(&str, Opener); 6] = [
    ("open", open_entry),
    ("recover", recover_entry),
    ("probe", probe_entry),
    ("root hold", root_hold_entry),
    ("recovery read", recovery_read_entry),
    ("read session", read_session_entry),
];

/// Runs every opener with the test lease port.
pub fn outcomes(root: &Path) -> Vec<(&'static str, Outcome)> {
    OPENERS
        .iter()
        .map(|(name, opener)| (*name, opener(root, common::lease::lease_port())))
        .collect()
}

/// Every opener refuses as `RecoveryInterrupted`; the storage openers say so exactly.
pub fn assert_all_interrupted(root: &Path, label: &str) {
    for (name, outcome) in outcomes(root) {
        assert!(outcome.is_interrupted(), "{label}: {name}: {outcome:?}");
    }
}

/// A copy of `template` (a flat store root) at `parent/name`.
pub fn copy_root(template: &Path, parent: &Path, name: &str) -> PathBuf {
    let root = parent.join(name);
    copy_tree(template, &root);
    root
}

/// The R7 move order of plan 3.6 for this root: `store.identity` first, then the sorted
/// inventory (locks, the record and `-shm` excluded), then `-shm` when present.
pub fn r7_order(root: &Path) -> Vec<String> {
    let excluded = ["store.lock", "io.lock", RECORD, "store.identity", SHM];
    let mut order = vec!["store.identity".to_string()];
    order.extend(
        names(root)
            .into_iter()
            .filter(|name| !excluded.contains(&name.as_str()) && !name.starts_with(".harflow")),
    );
    if root.join(SHM).exists() {
        order.push(SHM.to_string());
    }
    order
}

/// Moves `name` from `root` into `data` by rename, as R7 does.
pub fn move_out(root: &Path, data: &Path, name: &str) {
    fs::create_dir_all(data).unwrap();
    fs::rename(root.join(name), data.join(name)).unwrap();
}
