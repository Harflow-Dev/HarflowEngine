//! The maintenance session: proof that no cooperative owner holds the state root, and the
//! only place that may act on an unclean survey.
//!
//! The session creates `store.lock` and `io.lock` when absent (through the recovery
//! filesystem port, `O_EXCL`, so a racing creator is not an error), then takes `store.lock`,
//! `io.lock` and, when present, the older engine's `.harflow-store.lock`, each exclusively
//! without creating anything, retrying `Busy` every `LOCK_RETRY` within `LOCK_BUDGET`. Every
//! Rust participant holds `store.lock` shared for its life, so success proves there is none.
//! A budget that runs out is `Error::Busy` (reason `active_owner`); a root or lock that is
//! not the one the confirmation names, or whose listing changed while the locks were taken,
//! is `Error::Replaced` (stale).
#[cfg(test)]
use crate::survey;
use crate::{Error, Result, binding::Binding, small_file};
use hf_application::{
    ctl::state_recover::RecoveryFsPort,
    lease::{LeaseError, LeaseGuard, LeaseMode, LeasePort},
};
use hf_domain::capture::FileIdentity;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

/// Retry budget of each lock; the three locks can take about three times this in total.
pub const LOCK_BUDGET: Duration = Duration::from_secs(2);
/// Pause between attempts at a busy lock.
pub const LOCK_RETRY: Duration = Duration::from_millis(5);
const STORE_LOCK: &str = "store.lock";
const IO_LOCK: &str = "io.lock";
const LEGACY_LOCK: &str = ".harflow-store.lock";

/// Proof of the held locks. Only this module can build one, so only a session can run a
/// maintenance survey.
pub(crate) struct Authority(());

/// What the confirmation (or, on a resume, the journal) says about the root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRequest {
    pub root_identity: FileIdentity,
    /// The identity of `store.lock`, or `None` when the root had none when it was
    /// inspected (the lock is then created and any lock found is accepted).
    pub lock_identity: Option<FileIdentity>,
}

pub struct MaintenanceSession {
    pub(crate) root: PathBuf,
    pub(crate) leases: Arc<dyn LeasePort>,
    pub(crate) fs: Arc<dyn RecoveryFsPort>,
    pub(crate) authority: Authority,
    proof: Proof,
    _guards: Vec<Box<dyn LeaseGuard>>,
}

struct Proof {
    root: FileIdentity,
    store_lock: FileIdentity,
    io_lock: FileIdentity,
    store_created: bool,
    io_created: bool,
}

impl MaintenanceSession {
    /// `acquire_within` with the production budget.
    pub fn acquire(
        root: impl AsRef<Path>,
        request: &SessionRequest,
        fs: Arc<dyn RecoveryFsPort>,
        leases: Arc<dyn LeasePort>,
    ) -> Result<Self> {
        Self::acquire_within(root, request, fs, leases, LOCK_BUDGET)
    }

    /// Takes the exclusive locks of the root `request` names, each with its own `budget`.
    pub fn acquire_within(
        root: impl AsRef<Path>,
        request: &SessionRequest,
        fs: Arc<dyn RecoveryFsPort>,
        leases: Arc<dyn LeasePort>,
        budget: Duration,
    ) -> Result<Self> {
        let root = std::path::absolute(root.as_ref())?;
        let root_id = plain_root(&root)?;
        if root_id != request.root_identity {
            return Err(Error::Replaced);
        }
        let store_listed = listed(&root, STORE_LOCK)?;
        if let Some(expected) = &request.lock_identity
            && store_listed.as_ref() != Some(expected)
        {
            return Err(Error::Replaced);
        }
        let store_created = ensure(fs.as_ref(), &root, STORE_LOCK, store_listed.is_none())?;
        let io_created = ensure(
            fs.as_ref(),
            &root,
            IO_LOCK,
            listed(&root, IO_LOCK)?.is_none(),
        )?;
        let store_before = required(&root, STORE_LOCK)?;
        if let Some(expected) = &request.lock_identity
            && &store_before != expected
        {
            return Err(Error::Replaced);
        }
        let io_before = required(&root, IO_LOCK)?;
        let legacy_before = listed(&root, LEGACY_LOCK)?;
        let mut guards = vec![
            lock(leases.as_ref(), &root.join(STORE_LOCK), budget)?,
            lock(leases.as_ref(), &root.join(IO_LOCK), budget)?,
        ];
        if legacy_before.is_some() {
            guards.push(lock(leases.as_ref(), &root.join(LEGACY_LOCK), budget)?);
        }
        if plain_root(&root)? != root_id
            || listed(&root, STORE_LOCK)?.as_ref() != Some(&store_before)
            || listed(&root, IO_LOCK)?.as_ref() != Some(&io_before)
        {
            return Err(Error::Replaced);
        }
        match (&legacy_before, listed(&root, LEGACY_LOCK)?) {
            (None, Some(_)) => return Err(Error::Busy),
            (Some(before), now) if now.as_ref() != Some(before) => return Err(Error::Replaced),
            _ => {}
        }
        small_file::sync_dir(&root)?;
        Ok(Self {
            root,
            leases,
            fs,
            authority: Authority(()),
            proof: Proof {
                root: root_id,
                store_lock: store_before,
                io_lock: io_before,
                store_created,
                io_created,
            },
            _guards: guards,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn root_identity(&self) -> &FileIdentity {
        &self.proof.root
    }
    /// The inode of `store.lock` that was acquired; the journal records it.
    pub fn lock_identity(&self) -> &FileIdentity {
        &self.proof.store_lock
    }
    pub fn io_lock_identity(&self) -> &FileIdentity {
        &self.proof.io_lock
    }
    /// Whether this session created `store.lock`.
    pub fn store_lock_created(&self) -> bool {
        self.proof.store_created
    }
    /// Whether this session created `io.lock`.
    pub fn io_lock_created(&self) -> bool {
        self.proof.io_created
    }
    /// The lock identity to compare with a token's, as the reference does: a token that
    /// names no lock (the root had none when inspected) compares equal to whatever lock
    /// the first attempt or a retry created, so the token's null stands in; a token that
    /// names one is compared with the acquired inode. A resume compares with the journal,
    /// never with the null.
    pub fn comparable_lock_identity(&self, token: Option<&FileIdentity>) -> Option<FileIdentity> {
        token.map(|_| self.proof.store_lock.clone())
    }

    /// The root and both lock files are still the ones the session acquired.
    pub(crate) fn verify(&self) -> Result<()> {
        if plain_root(&self.root)? != self.proof.root
            || required(&self.root, STORE_LOCK)? != self.proof.store_lock
            || required(&self.root, IO_LOCK)? != self.proof.io_lock
        {
            return Err(Error::Replaced);
        }
        Ok(())
    }

    /// The binding of the store as it lies, captured between two checks of the session's
    /// proof, so its identities are the acquired ones; or the failure to bind it (a root
    /// with no marker or database has nothing to survey). A root that is no longer the
    /// acquired one is `Replaced`.
    pub(crate) fn bind(&self) -> Result<Binding> {
        self.verify()?;
        let binding = Binding::identify_existing(&self.root, self.leases.clone());
        self.verify()?;
        binding
    }

    /// The maintenance survey of the store as it lies; see `bind`. The proof is checked
    /// again after the survey.
    #[cfg(test)]
    pub(crate) fn survey(&self) -> Result<survey::Survey> {
        let binding = self.bind()?;
        let survey = survey::maintenance(&binding, &self.authority);
        self.verify()?;
        Ok(survey)
    }
}

/// Every existing component of `root` is a plain directory (no link); returns its identity.
fn plain_root(root: &Path) -> Result<FileIdentity> {
    let mut prefix = PathBuf::new();
    for component in root.components() {
        prefix.push(component);
        if lstat(&prefix)?.file_type().is_symlink() {
            return Err(Error::Replaced);
        }
    }
    let metadata = lstat(root)?;
    if !metadata.is_dir() {
        return Err(Error::Replaced);
    }
    identity(&metadata)
}

/// A missing path is stale; any other failure is its own error.
fn lstat(path: &Path) -> Result<fs::Metadata> {
    fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::Replaced
        } else {
            error.into()
        }
    })
}

/// The identity of the regular file `name`, `None` when absent, `InvalidRoot` otherwise.
fn listed(root: &Path, name: &str) -> Result<Option<FileIdentity>> {
    match fs::symlink_metadata(root.join(name)) {
        Ok(metadata) if metadata.is_file() => identity(&metadata).map(Some),
        Ok(_) => Err(Error::InvalidRoot),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn required(root: &Path, name: &str) -> Result<FileIdentity> {
    listed(root, name)?.ok_or(Error::Replaced)
}

/// Creates `name` when `absent`; an `EEXIST` race is not an error and reports `false`.
fn ensure(fs: &dyn RecoveryFsPort, root: &Path, name: &str, absent: bool) -> Result<bool> {
    if !absent {
        return Ok(false);
    }
    Ok(fs.create_lock(root, name)?)
}

/// Exclusive, `create=false`, retrying `Busy` every `LOCK_RETRY` until `budget` runs out.
fn lock(leases: &dyn LeasePort, path: &Path, budget: Duration) -> Result<Box<dyn LeaseGuard>> {
    let deadline = Instant::now() + budget;
    loop {
        match leases.acquire(path, LeaseMode::Exclusive, false, deadline) {
            Err(LeaseError::Busy) if Instant::now() + LOCK_RETRY < deadline => {
                std::thread::sleep(LOCK_RETRY);
            }
            Err(LeaseError::Busy | LeaseError::DeadlineExceeded) => return Err(Error::Busy),
            other => return other.map_err(crate::lease_error),
        }
    }
}

#[cfg(unix)]
fn identity(metadata: &fs::Metadata) -> Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

/// Non-unix roots never reach here: `Binding` refuses them first.
#[cfg(not(unix))]
fn identity(_: &fs::Metadata) -> Result<FileIdentity> {
    Err(Error::InvalidRoot)
}
