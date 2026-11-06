//! Fresh single-thread exec barrier; retained lease descriptors close only in exec.
use super::*;
use nix::{
    fcntl::FdFlag,
    sys::stat::{SFlag, fstat},
};
use std::{
    io,
    os::fd::{AsFd, AsRawFd},
    sync::MutexGuard,
};

/// Holds the coordinator lock until immediate exec (or a refusal return).
pub struct ExecLeaseBarrier {
    _state: MutexGuard<'static, State>,
}
pub fn barrier() -> io::Result<ExecLeaseBarrier> {
    if std::env::vars_os().next().is_some() {
        return Err(io::Error::other("exec ambient environment"));
    }
    let state = state()
        .try_lock()
        .map_err(|_| io::Error::other("exec lease busy"))?;
    if !state.quarantine.is_empty()
        || state
            .entries
            .values()
            .any(|entry| entry.writer || entry.readers != 0)
    {
        return Err(io::Error::other("exec active or quarantined lease"));
    }
    stdio(&std::io::stdin())?;
    stdio(&std::io::stdout())?;
    stdio(&std::io::stderr())?;
    let mut known = std::collections::BTreeSet::new();
    for (key, entry) in &state.entries {
        qualify(&entry.path, *key, entry.parent)
            .map_err(|_| io::Error::other("exec replaced lease"))?;
        if identity(&entry.file.metadata()?).ok() != Some(*key)
            || !FdFlag::from_bits_truncate(fcntl(&*entry.file, FcntlArg::F_GETFD)?)
                .contains(FdFlag::FD_CLOEXEC)
        {
            return Err(io::Error::other("exec sentinel identity or CLOEXEC"));
        }
        known.insert(entry.file.as_raw_fd());
    }
    descriptors(&known)?;
    Ok(ExecLeaseBarrier { _state: state })
}
fn stdio(fd: &impl AsFd) -> io::Result<()> {
    let metadata = fstat(fd)?;
    if SFlag::from_bits_truncate(metadata.st_mode) & SFlag::S_IFMT != SFlag::S_IFIFO
        || FdFlag::from_bits_truncate(fcntl(fd, FcntlArg::F_GETFD)?).contains(FdFlag::FD_CLOEXEC)
    {
        return Err(io::Error::other("exec stdio identity or CLOEXEC"));
    }
    Ok(())
}
fn descriptors(known: &std::collections::BTreeSet<i32>) -> io::Result<()> {
    let names = fs::read_dir("/dev/fd")?
        .map(|entry| {
            entry?
                .file_name()
                .into_string()
                .map_err(|_| io::Error::other("exec descriptor name"))
        })
        .collect::<io::Result<Vec<_>>>()?;
    for name in names {
        let number = name
            .parse::<i32>()
            .map_err(|_| io::Error::other("exec descriptor number"))?;
        if number <= 2 || known.contains(&number) {
            continue;
        }
        match fs::metadata(format!("/dev/fd/{number}")) {
            Err(e)
                if e.kind() == io::ErrorKind::NotFound
                    || e.raw_os_error() == Some(nix::errno::Errno::EBADF as i32) => {}
            Err(e) => return Err(e),
            Ok(_) => return Err(io::Error::other("exec foreign descriptor")),
        }
    }
    Ok(())
}
/// Private stdio uses NONBLOCK during handshake; vendor receives blocking pipes.
pub fn restore_blocking_stdio() -> io::Result<()> {
    blocking(&std::io::stdin())?;
    blocking(&std::io::stdout())?;
    blocking(&std::io::stderr())
}
fn blocking(fd: &impl AsFd) -> io::Result<()> {
    let mut flags = nix::fcntl::OFlag::from_bits_truncate(fcntl(fd, FcntlArg::F_GETFL)?);
    flags.remove(nix::fcntl::OFlag::O_NONBLOCK);
    fcntl(fd, FcntlArg::F_SETFL(flags))?;
    Ok(())
}

/// Post-exec fixed witness checks actual image inheritance before emitting bytes.
pub fn check_exec_image() -> io::Result<()> {
    let _barrier = barrier()?;
    require_blocking(&std::io::stdin())?;
    require_blocking(&std::io::stdout())?;
    require_blocking(&std::io::stderr())
}
fn require_blocking(fd: &impl AsFd) -> io::Result<()> {
    if nix::fcntl::OFlag::from_bits_truncate(fcntl(fd, FcntlArg::F_GETFL)?)
        .contains(nix::fcntl::OFlag::O_NONBLOCK)
    {
        return Err(io::Error::other("exec image nonblocking stdio"));
    }
    Ok(())
}
