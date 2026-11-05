use super::*;
use nix::fcntl::{FcntlArg, fcntl};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    mem::ManuallyDrop,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::PathBuf,
    sync::Mutex,
    time::Duration,
};
type Key = (u64, u64);
struct Entry {
    path: PathBuf,
    parent: Key,
    file: ManuallyDrop<File>,
    readers: usize,
    writer: bool,
}
#[derive(Default)]
struct State {
    entries: BTreeMap<Key, Entry>,
    quarantine: Vec<ManuallyDrop<File>>,
}
fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::default()))
}
fn identity(m: &fs::Metadata) -> Result<Key, LeaseError> {
    if !m.is_file() || m.nlink() != 1 || m.ino() == 0 {
        return Err(LeaseError::Invalid);
    }
    Ok((m.dev(), m.ino()))
}
fn physical(path: &Path) -> Result<PathBuf, LeaseError> {
    if !path.is_absolute() || path.as_os_str().len() > 4096 {
        return Err(LeaseError::Invalid);
    }
    let parent = path.parent().ok_or(LeaseError::Invalid)?;
    if fs::canonicalize(parent).map_err(|_| LeaseError::Invalid)? != parent {
        return Err(LeaseError::Invalid);
    }
    Ok(path.to_path_buf())
}
fn timely(deadline: Instant) -> Result<(), LeaseError> {
    if Instant::now() >= deadline {
        Err(LeaseError::DeadlineExceeded)
    } else {
        Ok(())
    }
}
pub(super) fn acquire(
    path: &Path,
    mode: LeaseMode,
    create: bool,
    deadline: Instant,
) -> Result<Box<dyn LeaseGuard>, LeaseError> {
    let deadline = deadline.min(Instant::now() + Duration::from_secs(2));
    timely(deadline)?;
    let mut state = state().try_lock().map_err(|_| LeaseError::Busy)?;
    let path = physical(path)?;
    let parent_metadata =
        fs::metadata(path.parent().ok_or(LeaseError::Invalid)?).map_err(|_| LeaseError::Invalid)?;
    let parent = (parent_metadata.dev(), parent_metadata.ino());
    for (key, entry) in &state.entries {
        if entry.path == path
            && (entry.parent != parent
                || fs::symlink_metadata(&path)
                    .ok()
                    .and_then(|m| identity(&m).ok())
                    != Some(*key))
        {
            return Err(LeaseError::Replaced);
        }
    }
    let existing = match fs::symlink_metadata(&path) {
        Ok(m) => Some(identity(&m)?),
        Err(e) if create && e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(LeaseError::Invalid),
    };
    if existing
        .and_then(|k| state.entries.get(&k))
        .is_some_and(|entry| entry.path != path)
    {
        return Err(LeaseError::Replaced);
    }
    let key = if let Some(key) = existing.filter(|k| state.entries.contains_key(k)) {
        key
    } else {
        open(&mut state, &path, parent, create, deadline)?
    };
    let entry = state.entries.get_mut(&key).ok_or(LeaseError::Unavailable)?;
    if identity(&fs::symlink_metadata(&path).map_err(|_| LeaseError::Replaced)?)? != key {
        return Err(LeaseError::Replaced);
    }
    let parent_now = fs::metadata(path.parent().ok_or(LeaseError::Invalid)?)
        .map_err(|_| LeaseError::Replaced)?;
    if physical(&path)? != path
        || entry.parent != parent
        || parent != (parent_now.dev(), parent_now.ino())
    {
        return Err(LeaseError::Replaced);
    }
    timely(deadline)?;
    match mode {
        LeaseMode::Shared if entry.writer => return Err(LeaseError::Busy),
        LeaseMode::Shared if entry.readers >= 1024 => return Err(LeaseError::LimitExceeded),
        LeaseMode::Exclusive if entry.writer || entry.readers != 0 => return Err(LeaseError::Busy),
        _ => (),
    }
    if entry.readers == 0 {
        lock(&entry.file, mode)?;
    }
    match mode {
        LeaseMode::Shared => entry.readers += 1,
        LeaseMode::Exclusive => entry.writer = true,
    }
    let guard = Box::new(Guard { key, mode });
    let qualified = qualify(&path, key, parent);
    drop(state);
    if let Err(error) = qualified.and_then(|()| timely(deadline)) {
        drop(guard);
        return Err(error);
    }
    Ok(guard)
}
fn open(
    state: &mut State,
    path: &Path,
    parent: Key,
    create: bool,
    deadline: Instant,
) -> Result<Key, LeaseError> {
    if state.entries.len() + state.quarantine.len() >= 256 {
        return Err(LeaseError::LimitExceeded);
    }
    timely(deadline)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK | nix::libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| LeaseError::Invalid)?;
    state.quarantine.push(ManuallyDrop::new(file));
    let file = state.quarantine.last().ok_or(LeaseError::Unavailable)?;
    let key = identity(&file.metadata().map_err(|_| LeaseError::Invalid)?)?;
    if identity(&fs::symlink_metadata(path).map_err(|_| LeaseError::Replaced)?)? != key
        || physical(path)? != path
    {
        return Err(LeaseError::Replaced);
    }
    if state
        .entries
        .get(&key)
        .is_some_and(|entry| entry.path != path)
    {
        return Err(LeaseError::Replaced);
    }
    if !state.entries.contains_key(&key) {
        let metadata = fs::metadata(path.parent().ok_or(LeaseError::Invalid)?)
            .map_err(|_| LeaseError::Invalid)?;
        if parent != (metadata.dev(), metadata.ino()) {
            return Err(LeaseError::Replaced);
        }
        let file = state.quarantine.pop().ok_or(LeaseError::Unavailable)?;
        state.entries.insert(
            key,
            Entry {
                path: path.to_path_buf(),
                parent: (metadata.dev(), metadata.ino()),
                file,
                readers: 0,
                writer: false,
            },
        );
    }
    Ok(key)
}
fn qualify(path: &Path, key: Key, parent: Key) -> Result<(), LeaseError> {
    let metadata = fs::metadata(path.parent().ok_or(LeaseError::Invalid)?)
        .map_err(|_| LeaseError::Replaced)?;
    if physical(path)? != path
        || parent != (metadata.dev(), metadata.ino())
        || identity(&fs::symlink_metadata(path).map_err(|_| LeaseError::Replaced)?)? != key
    {
        return Err(LeaseError::Replaced);
    }
    Ok(())
}
fn lock(file: &File, mode: LeaseMode) -> Result<(), LeaseError> {
    set(
        file,
        match mode {
            LeaseMode::Shared => nix::libc::F_RDLCK,
            LeaseMode::Exclusive => nix::libc::F_WRLCK,
        },
    )
}
fn set(file: &File, kind: i16) -> Result<(), LeaseError> {
    let record = nix::libc::flock {
        l_start: 0,
        l_len: 0,
        l_pid: 0,
        l_type: kind,
        l_whence: nix::libc::SEEK_SET as i16,
    };
    fcntl(file, FcntlArg::F_SETLK(&record))
        .map(|_| ())
        .map_err(|e| match e {
            nix::errno::Errno::EACCES | nix::errno::Errno::EAGAIN => LeaseError::Busy,
            _ => LeaseError::Unavailable,
        })
}
struct Guard {
    key: Key,
    mode: LeaseMode,
}
impl LeaseGuard for Guard {}
impl Drop for Guard {
    fn drop(&mut self) {
        // Release must proceed even after poison; critical sections never wait on I/O locks.
        let mut state = match state().lock() {
            Ok(s) => s,
            Err(e) => e.into_inner(),
        };
        if let Some(entry) = state.entries.get_mut(&self.key) {
            match self.mode {
                LeaseMode::Shared => entry.readers = entry.readers.saturating_sub(1),
                LeaseMode::Exclusive => entry.writer = false,
            }
            if entry.readers == 0 && !entry.writer && set(&entry.file, nix::libc::F_UNLCK).is_err()
            {
                // Unknown kernel state must never authorize another local holder.
                entry.writer = true;
            }
        }
    }
}

#[cfg(test)]
#[path = "darwin/tests.rs"]
mod lease_tests;
