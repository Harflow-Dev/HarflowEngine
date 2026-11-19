//! Files-only initialization under the caller's existing exclusion and directory proof.
use crate::{Identity, Result, connection, small_file, wal, watermark};
use hf_application::durability::FlushPort;
use rusqlite::{Connection, OpenFlags};
use std::{path::Path, time::Instant};

/// Creates only the fixed store files in an existing, empty directory. The caller
/// owns its directory identity proof and all leases. No Store handle escapes, and
/// the connection is closed before return, including every failure path.
pub fn initialize_files(
    existing_dir: &Path,
    flush: &dyn FlushPort,
    deadline: Instant,
    verify: &dyn Fn() -> Result<()>,
) -> Result<Identity> {
    initialize(
        existing_dir,
        flush,
        deadline,
        verify,
        connection::InitialCheckpoint::KeepFrames,
    )
}

#[cfg(test)]
pub(crate) fn fixture_files(
    existing_dir: &Path,
    flush: &dyn FlushPort,
    deadline: Instant,
    verify: &dyn Fn() -> Result<()>,
) -> Result<Identity> {
    new_root_files(existing_dir, flush, deadline, verify)
}
/// Neutral initializer used by fresh production namespaces and closed fixture wrappers.
pub(crate) fn new_root_files(
    existing_dir: &Path,
    flush: &dyn FlushPort,
    deadline: Instant,
    verify: &dyn Fn() -> Result<()>,
) -> Result<Identity> {
    initialize(
        existing_dir,
        flush,
        deadline,
        verify,
        connection::InitialCheckpoint::Truncate,
    )
}

fn initialize(
    existing_dir: &Path,
    flush: &dyn FlushPort,
    deadline: Instant,
    verify: &dyn Fn() -> Result<()>,
    checkpoint: connection::InitialCheckpoint,
) -> Result<Identity> {
    connection::check_deadline(deadline)?;
    verify()?;
    let mut connection = Connection::open_with_flags(
        existing_dir.join("store.sqlite3"),
        OpenFlags::default() | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    connection::suppress_checkpoints(&connection)?;
    verify()?;
    connection::configure(&connection, deadline)?;
    connection::initialize(&mut connection, deadline, checkpoint)?;
    let identity = connection::identity(&connection, deadline)?;
    connection::check_deadline(deadline)?;
    verify()?;
    watermark::create(existing_dir, &identity.generation, flush)?;
    small_file::sync_dir(existing_dir)?;
    wal::publish(existing_dir, &identity)?;
    small_file::sync_dir(existing_dir)?;
    drop(connection);
    verify()?;
    Ok(identity)
}

/// Creates the root directory with no group or other access (mode 0700 before the umask on
/// unix). The caller treats an existing path as `AlreadyExists`; no parent is created.
pub(crate) fn create_private_root(root: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(root)
}
