use nix::sys::stat::{SFlag, fstat};
use std::{fs, io, os::fd::AsFd};
/// Call only in the fresh dedicated single-thread fixture host, at entry and
/// immediately before spawn. This is not a concurrent-process launch barrier.
pub fn check_entry() -> io::Result<()> {
    if std::env::vars_os().next().is_some() {
        return Err(io::Error::other("fixture ambient environment"));
    }
    require_pipe(&std::io::stdin())?;
    require_pipe(&std::io::stdout())?;
    require_pipe(&std::io::stderr())?;
    let names: Vec<String> = fs::read_dir("/dev/fd")?
        .map(|entry| {
            entry?
                .file_name()
                .into_string()
                .map_err(|_| io::Error::other("fixture descriptor name"))
        })
        .collect::<io::Result<_>>()?;
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

fn require_pipe(fd: &impl AsFd) -> io::Result<()> {
    let metadata = fstat(fd)?;
    if SFlag::from_bits_truncate(metadata.st_mode) & SFlag::S_IFMT != SFlag::S_IFIFO {
        return Err(io::Error::other("fixture stdio is not a private pipe"));
    }
    Ok(())
}
