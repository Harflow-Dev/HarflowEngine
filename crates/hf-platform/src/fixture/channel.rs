use nix::{
    fcntl::{FcntlArg, OFlag, fcntl},
    poll::{PollFd, PollFlags, PollTimeout, poll},
    unistd,
};
use std::{io, os::fd::AsFd, time::Instant};
const MAX_FRAME: usize = 24576;
pub(super) fn ready(fd: &impl AsFd, event: PollFlags, deadline: Instant) -> io::Result<()> {
    let left = deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "fixture deadline"))?;
    let timeout = PollTimeout::try_from(left).map_err(|_| io::Error::other("fixture deadline"))?;
    let mut fds = [PollFd::new(fd.as_fd(), event)];
    match poll(&mut fds, timeout) {
        Ok(0) => Err(io::Error::new(io::ErrorKind::TimedOut, "fixture deadline")),
        Ok(_) => check_deadline(deadline),
        Err(e) => Err(io::Error::from(e)),
    }
}
fn nonblocking(fd: &impl AsFd) -> io::Result<()> {
    let flags = OFlag::from_bits_truncate(fcntl(fd, FcntlArg::F_GETFL)?);
    fcntl(fd, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
    Ok(())
}
fn exact(fd: &impl AsFd, bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
    let mut at = 0;
    while at < bytes.len() {
        ready(fd, PollFlags::POLLIN, deadline)?;
        check_deadline(deadline)?;
        let result = unistd::read(fd, &mut bytes[at..]);
        check_deadline(deadline)?;
        match result {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "fixture truncated",
                ));
            }
            Ok(n) => at += n,
            Err(nix::errno::Errno::EINTR | nix::errno::Errno::EAGAIN) => {}
            Err(e) => return Err(e.into()),
        }
    }
    check_deadline(deadline)
}
/// Reads one length-prefixed frame, rejecting its size before allocating its body.
pub fn read_frame(fd: &impl AsFd, deadline: Instant) -> io::Result<Vec<u8>> {
    check_deadline(deadline)?;
    nonblocking(fd)?;
    let mut prefix = [0; 4];
    exact(fd, &mut prefix, deadline)?;
    let n = u32::from_be_bytes(prefix) as usize;
    if !(6..=MAX_FRAME - 4).contains(&n) {
        return Err(io::Error::other("fixture frame length"));
    }
    let mut bytes = vec![0; n + 4];
    bytes[..4].copy_from_slice(&prefix);
    exact(fd, &mut bytes[4..], deadline)?;
    check_deadline(deadline)?;
    Ok(bytes)
}
pub fn require_eof(fd: &impl AsFd, deadline: Instant) -> io::Result<()> {
    check_deadline(deadline)?;
    nonblocking(fd)?;
    loop {
        ready(fd, PollFlags::POLLIN, deadline)?;
        check_deadline(deadline)?;
        let result = unistd::read(fd, &mut [0; 1]);
        check_deadline(deadline)?;
        match result {
            Ok(0) => return check_deadline(deadline),
            Ok(_) => return Err(io::Error::other("fixture trailing input")),
            Err(nix::errno::Errno::EINTR | nix::errno::Errno::EAGAIN) => {}
            Err(e) => return Err(e.into()),
        }
    }
}
pub fn write_all(fd: &impl AsFd, bytes: &[u8], deadline: Instant) -> io::Result<()> {
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::other("fixture frame length"));
    }
    check_deadline(deadline)?;
    nonblocking(fd)?;
    let mut at = 0;
    while at < bytes.len() {
        ready(fd, PollFlags::POLLOUT, deadline)?;
        check_deadline(deadline)?;
        let result = unistd::write(fd, &bytes[at..]);
        check_deadline(deadline)?;
        match result {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "fixture write")),
            Ok(n) => at += n,
            Err(nix::errno::Errno::EINTR | nix::errno::Errno::EAGAIN) => {}
            Err(e) => return Err(e.into()),
        }
    }
    check_deadline(deadline)
}

/// Owns the one absolute local deadline for inherited unbuffered stdio.
pub struct PrivateIo {
    deadline: Instant,
}
impl PrivateIo {
    pub fn stdio(deadline: Instant) -> Self {
        Self { deadline }
    }
    pub fn read_packet(&self) -> io::Result<Vec<u8>> {
        read_frame(&std::io::stdin(), self.deadline)
    }
    pub fn write_packet(&self, bytes: &[u8]) -> io::Result<()> {
        write_all(&std::io::stdout(), bytes, self.deadline)
    }
    pub fn expect_eof(&self) -> io::Result<()> {
        require_eof(&std::io::stdin(), self.deadline)
    }
}

fn check_deadline(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "fixture deadline"));
    }
    Ok(())
}
