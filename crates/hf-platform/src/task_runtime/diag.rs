//! Best-effort diagnostic lines for the owner and writer processes.
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::poll::{PollFd, PollFlags, poll};
use std::os::fd::{AsFd, BorrowedFd};

/// The longest line written; one write of this size to a pipe is atomic.
pub const DIAG_LIMIT: usize = 256;
/// Writes one diagnostic line to stderr. It briefly sets O_NONBLOCK on the descriptor (an
/// open file description that may be shared, so it is meant for the owner's or writer's
/// dedicated stderr), makes one write attempt, only when a poll with no wait reports it
/// writable, and restores the original flags. It ignores every failure: a closed or full
/// pipe, a short write, EPIPE and a missing descriptor. It never retries, blocks, panics or
/// prints through the standard macros. SIGPIPE is ignored by the Rust runtime, so a closed
/// pipe is an EPIPE, not a signal.
pub fn diag_line(line: &[u8]) {
    diag_line_to(std::io::stderr().as_fd(), line);
}
/// `diag_line` for an explicit descriptor.
pub fn diag_line_to(fd: BorrowedFd<'_>, line: &[u8]) {
    if line.len() > DIAG_LIMIT {
        return;
    }
    // A poll alone cannot stop another writer that shares the pipe from filling it before
    // the write, so the write itself is made non-blocking. Skip on failure.
    let Ok(raw) = fcntl(fd, FcntlArg::F_GETFL) else {
        return;
    };
    let original = OFlag::from_bits_truncate(raw);
    if !original.contains(OFlag::O_NONBLOCK)
        && fcntl(fd, FcntlArg::F_SETFL(original | OFlag::O_NONBLOCK)).is_err()
    {
        return;
    }
    write_once(fd, line);
    if !original.contains(OFlag::O_NONBLOCK) {
        let _ = fcntl(fd, FcntlArg::F_SETFL(original));
    }
}
fn write_once(fd: BorrowedFd<'_>, line: &[u8]) {
    let mut set = [PollFd::new(fd, PollFlags::POLLOUT)];
    if !matches!(poll(&mut set, 0u16), Ok(count) if count > 0) {
        return;
    }
    let Some(events) = set[0].revents() else {
        return;
    };
    let broken = PollFlags::POLLERR | PollFlags::POLLHUP | PollFlags::POLLNVAL;
    if events.contains(PollFlags::POLLOUT) && !events.intersects(broken) {
        let _ = nix::unistd::write(fd, line);
    }
}
