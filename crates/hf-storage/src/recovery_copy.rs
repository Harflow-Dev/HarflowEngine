//! Capped private main+WAL copy qualification, kept only for `RecoveryReadSession`.
//! Writer open, recover and probe qualify in place (`qualify.rs`) and never copy.
use crate::{
    Error, Identity, Result,
    binding::Binding,
    connection,
    maintenance_copy::{Ident, ident, same_dir},
    wal::{marker, reject_nonempty_journal},
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
const CAP: u64 = 64 * 1024 * 1024;
static SERIAL: AtomicU64 = AtomicU64::new(0);
/// The private directory holding the copy. Its device/inode is recorded when created and
/// verified (a real directory, not a link) immediately before the recursive removal; a
/// mismatch is `Replaced` from `cleanup` and a silent no-op in `Drop`. Residual: the
/// instant between that check and `remove_dir_all` is not excluded (path-based removal).
pub(crate) struct Scratch(PathBuf, Ident);
impl Drop for Scratch {
    fn drop(&mut self) {
        if same_dir(&self.0, self.1).is_ok() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
impl Scratch {
    pub(crate) fn cleanup(&self) -> Result<()> {
        match fs::symlink_metadata(&self.0) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            _ => {}
        }
        same_dir(&self.0, self.1)?;
        fs::remove_dir_all(&self.0)?;
        Ok(())
    }
    pub(crate) fn create(parent: &Path) -> Result<Self> {
        Binding::validate_probe_path(parent)?;
        #[cfg(unix)]
        use std::os::unix::fs::DirBuilderExt;
        for _ in 0..32 {
            let path = parent.join(format!(
                ".hf-snapshot-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            #[cfg_attr(not(unix), allow(unused_mut))]
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => {
                    return match fs::symlink_metadata(&path) {
                        Ok(metadata) => Ok(Self(path, ident(&metadata))),
                        Err(error) => {
                            let _ = fs::remove_dir(&path);
                            Err(error.into())
                        }
                    };
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(Error::Busy)
    }
}
fn copy(source: &Path, target: &Path, remaining: &mut u64, deadline: Instant) -> Result<()> {
    let mut input = File::open(source)?;
    let size = input.metadata()?.len();
    if size > *remaining {
        return Err(Error::LimitExceeded);
    }
    *remaining -= size;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    let mut buffer = [0; 64 * 1024];
    let mut copied = 0;
    loop {
        connection::check_deadline(deadline)?;
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        copied += count as u64;
        if copied > size {
            return Err(Error::Replaced);
        }
        output.write_all(&buffer[..count])?;
    }
    if copied != size {
        return Err(Error::Replaced);
    }
    Ok(())
}
pub(crate) fn with_qualified_copy<T>(
    binding: &Binding,
    parent: &Path,
    deadline: Instant,
    action: impl FnOnce(&rusqlite::Connection, &Identity) -> Result<T>,
) -> Result<T> {
    connection::check_deadline(deadline)?;
    binding.check()?;
    reject_nonempty_journal(binding)?;
    let expected = marker(binding)?;
    Binding::validate_probe_path(parent)?;
    let parent = fs::canonicalize(parent)?;
    if parent.starts_with(fs::canonicalize(binding.root())?) {
        return Err(Error::InvalidRoot);
    }
    let scratch = Scratch::create(&parent)?;
    let result = qualify_copy(binding, &scratch, expected, deadline, action);
    scratch.cleanup()?;
    result
}
fn qualify_copy<T>(
    binding: &Binding,
    scratch: &Scratch,
    expected: Identity,
    deadline: Instant,
    action: impl FnOnce(&rusqlite::Connection, &Identity) -> Result<T>,
) -> Result<T> {
    let database = scratch.0.join("store.sqlite3");
    let mut remaining = CAP;
    copy(&binding.database(), &database, &mut remaining, deadline)?;
    let source_wal = binding.root().join("store.sqlite3-wal");
    if source_wal.try_exists()? {
        copy(
            &source_wal,
            &scratch.0.join("store.sqlite3-wal"),
            &mut remaining,
            deadline,
        )?;
    }
    binding.check()?;
    connection::qualify_header(&database)?;
    let connection = rusqlite::Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    )?;
    connection::suppress_checkpoints(&connection)?;
    let observed = connection::identity(&connection, deadline)?;
    connection::timeout(&connection, deadline)?;
    let integrity: String = connection.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))?;
    if integrity != "ok" || observed != expected {
        return Err(Error::Incompatible);
    }
    connection::check_deadline(deadline)?;
    let result = action(&connection, &observed)?;
    binding.check()?;
    if marker(binding)? != expected {
        return Err(Error::Replaced);
    }
    connection::check_deadline(deadline)?;
    Ok(result)
}
