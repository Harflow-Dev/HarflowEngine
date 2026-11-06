use std::io::{self, Read};
use std::os::fd::AsFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::time::Instant;

pub(super) fn read_helper(path: &std::path::Path) -> io::Result<Vec<u8>> {
    let before = std::fs::symlink_metadata(path)?;
    if !before.is_file()
        || before.nlink() != 1
        || before.len() > 1_048_576
        || before.mode() & 0o022 != 0
        || before.mode() & 0o111 == 0
    {
        return Err(io::Error::other("helper identity/mode/budget refused"));
    }
    let mut bytes = Vec::new();
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    let opened = file.metadata()?;
    if opened.dev() != before.dev() || opened.ino() != before.ino() {
        return Err(io::Error::other("helper path changed at open"));
    }
    file.take(1_048_577).read_to_end(&mut bytes)?;
    let after = std::fs::symlink_metadata(path)?;
    if bytes.len() > 1_048_576
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
    {
        return Err(io::Error::other("helper changed while observed"));
    }
    Ok(bytes)
}

pub(super) fn witness_unchanged(path: &std::path::Path) -> io::Result<bool> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.nlink() != 1 || meta.len() != 18 {
        return Err(io::Error::other("witness identity/extent unknown"));
    }
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    let opened = file.metadata()?;
    if opened.dev() != meta.dev() || opened.ino() != meta.ino() {
        return Err(io::Error::other("witness changed at open"));
    }
    let mut bytes = Vec::new();
    file.by_ref().take(19).read_to_end(&mut bytes)?;
    let after = std::fs::symlink_metadata(path)?;
    Ok(bytes == b"ST04-READ-WITNESS\n"
        && file.metadata()?.len() == 18
        && after.dev() == meta.dev()
        && after.ino() == meta.ino()
        && after.len() == meta.len()
        && after.nlink() == 1)
}
pub(super) fn network_receipt(
    listener: &std::net::TcpListener,
    deadline: Instant,
) -> io::Result<Option<Vec<u8>>> {
    let mut connection = match listener.accept() {
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
        Ok((connection, _)) => connection,
        Err(error) => return Err(error),
    };
    connection.set_nonblocking(true)?;
    let mut bytes = Vec::with_capacity(14);
    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::other("network receipt unknown"));
        }
        let mut buffer = [0_u8; 14];
        match connection.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) if bytes.len() + count <= 13 => bytes.extend_from_slice(&buffer[..count]),
            Ok(_) => return Err(io::Error::other("network receipt overflow")),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let mut fds = [nix::poll::PollFd::new(
                    connection.as_fd(),
                    nix::poll::PollFlags::POLLIN,
                )];
                let millis = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(20) as u16;
                match nix::poll::poll(&mut fds, millis) {
                    Ok(_) | Err(nix::errno::Errno::EINTR) => (),
                    Err(error) => return Err(error.into()),
                }
            }
            Err(error) => return Err(error),
        }
    }
    if bytes != b"ST04-NETWORK\n" {
        return Err(io::Error::other("network marker mismatch"));
    }
    Ok(Some(bytes))
}
