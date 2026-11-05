use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::poll::{PollFd, PollFlags, poll};
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::AsFd;
use std::time::Instant;
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StreamCapture {
    pub bytes: Vec<u8>,
    pub eof: bool,
    pub overflow: bool,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Capture {
    pub stdout: StreamCapture,
    pub stderr: StreamCapture,
}
pub(super) fn nonblocking(fd: &impl AsFd) -> io::Result<()> {
    let flags = fcntl(fd, FcntlArg::F_GETFL).map_err(io::Error::from)?;
    fcntl(
        fd,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .map_err(io::Error::from)?;
    Ok(())
}
fn read_stream(
    pipe: &mut impl Read,
    state: &mut StreamCapture,
    cap: usize,
    budget: usize,
    deadline: Instant,
) {
    let mut buffer = [0_u8; 4096];
    let mut remaining = budget;
    while remaining > 0 && Instant::now() < deadline && !state.eof && state.error.is_none() {
        let count = remaining.min(buffer.len());
        match pipe.read(&mut buffer[..count]) {
            Ok(0) => state.eof = true,
            Ok(n) => {
                remaining -= n;
                let keep = n.min(cap.saturating_sub(state.bytes.len()));
                state.bytes.extend_from_slice(&buffer[..keep]);
                state.overflow |= keep < n;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => state.error = Some(error.to_string()),
        }
    }
}
pub(super) fn pump(
    out: &mut File,
    err: &mut File,
    state: &mut Capture,
    cap: usize,
    budget: usize,
    deadline: Instant,
) -> io::Result<()> {
    if Instant::now() >= deadline {
        return Ok(());
    }
    let mut descriptors = Vec::with_capacity(2);
    if !state.stdout.eof && state.stdout.error.is_none() {
        descriptors.push(PollFd::new(out.as_fd(), PollFlags::POLLIN));
    }
    if !state.stderr.eof && state.stderr.error.is_none() {
        descriptors.push(PollFd::new(err.as_fd(), PollFlags::POLLIN));
    }
    let millis = deadline
        .saturating_duration_since(Instant::now())
        .as_millis()
        .min(20) as u16;
    match poll(&mut descriptors, millis) {
        Ok(_) => (),
        Err(nix::errno::Errno::EINTR) => return Ok(()),
        Err(error) => return Err(io::Error::from(error)),
    }
    // Each stream receives a fixed share, including when the other stream stays busy.
    read_stream(out, &mut state.stdout, cap, budget / 2, deadline);
    read_stream(err, &mut state.stderr, cap, budget / 2, deadline);
    Ok(())
}
