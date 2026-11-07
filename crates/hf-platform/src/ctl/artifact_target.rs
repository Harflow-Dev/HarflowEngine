//! Observes where an artifact reference points now, from descriptors only: nothing is read
//! from a file, nothing is written and no component is followed after canonicalization.
use super::{
    artifact_authority::{Shared, authorized},
    kernel_spelling::{Spelling, kernel_spelling},
};
use hf_application::ctl::task_reads::ArtifactTargetPort;
use hf_domain::ctl::history::{ArtifactItem, Availability, Observation};
use nix::{
    fcntl::{OFlag, open, openat},
    libc,
    sys::stat::{Mode, fstat},
    unistd::{AccessFlags, Uid, User, access},
};
use std::{
    os::fd::OwnedFd,
    path::{Component, Path},
};

/// The only registration this engine can judge: a process the task's lineage verified.
const SUPPORTED_METHOD: &str = "verified_process_lineage";

/// The account name of the real uid (`getpwuid`), or the text of the `KeyError` Python
/// raises when there is none.
pub fn system_account() -> Result<String, String> {
    let uid = Uid::current();
    match User::from_uid(uid) {
        Ok(Some(user)) => Ok(user.name),
        _ => Err(format!("'getpwuid(): uid not found: {}'", uid.as_raw())),
    }
}

/// Observes targets for one command. What does not depend on the repository is resolved on
/// the first artifact that needs it and kept, failure included. A missing account ends the
/// whole command, which the use case learns from `command_failure`.
pub struct CommandArtifactTargets {
    account: Box<dyn Fn() -> Result<String, String>>,
    shared: Shared,
}

impl CommandArtifactTargets {
    pub fn new() -> Self {
        Self::with_account(system_account)
    }

    /// The account name comes from `account`, which is read at most once.
    pub fn with_account(account: impl Fn() -> Result<String, String> + 'static) -> Self {
        Self {
            account: Box::new(account),
            shared: Shared::default(),
        }
    }
}

impl Default for CommandArtifactTargets {
    fn default() -> Self {
        Self::new()
    }
}

fn directory_flags() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC
}

/// Metadata-oriented, non-blocking: the descriptor is only ever `fstat`ed.
fn final_flags() -> OFlag {
    OFlag::from_bits_retain(libc::O_EVTONLY)
        | OFlag::O_NOFOLLOW
        | OFlag::O_NONBLOCK
        | OFlag::O_CLOEXEC
}

/// `fstat` of a regular file reached through no-follow directory descriptors, one per
/// component, so an intermediate directory swapped for a symlink fails the walk.
fn regular_stat(path: &Path) -> Option<Observation> {
    let mut directory: OwnedFd = open("/", directory_flags(), Mode::empty()).ok()?;
    let names: Vec<_> = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();
    let (last, parents) = names.split_last()?;
    for name in parents {
        directory = openat(&directory, *name, directory_flags(), Mode::empty()).ok()?;
    }
    let file = openat(&directory, *last, final_flags(), Mode::empty()).ok()?;
    let stat = fstat(&file).ok()?;
    if u32::from(stat.st_mode) & u32::from(libc::S_IFMT) != u32::from(libc::S_IFREG) {
        return None;
    }
    let nanos = i128::from(stat.st_mtime) * 1_000_000_000 + i128::from(stat.st_mtime_nsec);
    Some(Observation {
        device: stat.st_dev as u64,
        inode: stat.st_ino,
        size: u64::try_from(stat.st_size).ok()?,
        mtime_ns: i64::try_from(nanos).ok()?,
    })
}

impl ArtifactTargetPort for CommandArtifactTargets {
    fn availability(&self, repo_path: &str, item: &ArtifactItem) -> Availability {
        let registered = Path::new(&item.path);
        // Only the absence of the path itself establishes deletion.
        let canonical = match kernel_spelling(registered, true) {
            Some(Spelling::Found(path)) => path,
            Some(Spelling::Missing) => return Availability::Deleted,
            None => return Availability::Unavailable,
        };
        if canonical != registered {
            return Availability::Changed;
        }
        // Engine output and observed plans need facts this engine does not store yet.
        if item.method != SUPPORTED_METHOD {
            return Availability::Unavailable;
        }
        if !authorized(&self.shared, &*self.account, repo_path, &canonical) {
            return Availability::Unavailable;
        }
        let Some(observed) = regular_stat(&canonical) else {
            return Availability::Unavailable;
        };
        if access(&canonical, AccessFlags::R_OK).is_err() {
            Availability::Unavailable
        } else if observed == item.registered {
            Availability::Available
        } else {
            Availability::Changed
        }
    }

    fn command_failure(&self) -> Option<String> {
        self.shared.command_failure()
    }
}
