use nix::{
    fcntl::{FcntlArg, OFlag, fcntl},
    sys::stat::{SFlag, fstat},
};
use std::{fs, io, os::fd::AsFd};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PipeIdentity {
    device: i32,
    inode: u64,
}
pub(super) fn pipe_identity(fd: &impl AsFd) -> io::Result<PipeIdentity> {
    let metadata = fstat(fd)?;
    if SFlag::from_bits_truncate(metadata.st_mode) & SFlag::S_IFMT != SFlag::S_IFIFO {
        return Err(io::Error::other("fixture stdio is not a private pipe"));
    }
    Ok(PipeIdentity {
        device: metadata.st_dev,
        inode: metadata.st_ino,
    })
}
/// Fresh single-thread entry only, before any deliberate descriptor acquisition.
pub(crate) fn check_empty_entry(strict_access: bool) -> io::Result<()> {
    if std::env::vars_os().next().is_some() {
        return Err(io::Error::other("fixture ambient environment"));
    }
    pipe_identity(&std::io::stdin())?;
    pipe_identity(&std::io::stdout())?;
    pipe_identity(&std::io::stderr())?;
    if strict_access {
        let modes = [
            fcntl(std::io::stdin(), FcntlArg::F_GETFL)?,
            fcntl(std::io::stdout(), FcntlArg::F_GETFL)?,
            fcntl(std::io::stderr(), FcntlArg::F_GETFL)?,
        ];
        if modes[0] & OFlag::O_ACCMODE.bits() != OFlag::O_RDONLY.bits()
            || modes[1..]
                .iter()
                .any(|mode| mode & OFlag::O_ACCMODE.bits() != OFlag::O_WRONLY.bits())
        {
            return Err(io::Error::other("bootstrap pipe direction refused"));
        }
    }
    let names = fs::read_dir("/dev/fd")?
        .take(257)
        .map(|entry| {
            entry?
                .file_name()
                .into_string()
                .map_err(|_| io::Error::other("fixture descriptor name"))
        })
        .collect::<io::Result<Vec<_>>>()?;
    if names.len() > 256 {
        return Err(io::Error::other("bootstrap descriptor inventory truncated"));
    }
    for name in names {
        let number = name
            .parse::<u32>()
            .map_err(|_| io::Error::other("fixture descriptor name"))?;
        if number <= 2 {
            continue;
        }
        match fs::metadata(format!("/dev/fd/{number}")) {
            Err(e)
                if e.kind() == io::ErrorKind::NotFound
                    || e.raw_os_error() == Some(nix::errno::Errno::EBADF as i32) => {}
            Err(e) => return Err(e),
            Ok(_) => return Err(io::Error::other("fixture inherited descriptor")),
        }
    }
    Ok(())
}
