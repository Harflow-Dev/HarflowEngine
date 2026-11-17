//! Historical fixture framing limits over the neutral private channel mechanism.
pub(super) use crate::task_runtime::channel::blocking::ready;
use std::{io, os::fd::AsFd, time::Instant};
const MAX_FRAME: usize = 24576;
pub fn read_frame(fd: &impl AsFd, deadline: Instant) -> io::Result<Vec<u8>> {
    crate::task_runtime::channel::blocking::read_frame(fd, deadline, MAX_FRAME)
}
pub fn write_all(fd: &impl AsFd, bytes: &[u8], deadline: Instant) -> io::Result<()> {
    crate::task_runtime::channel::blocking::write_all(fd, bytes, deadline, MAX_FRAME)
}
pub fn require_eof(fd: &impl AsFd, deadline: Instant) -> io::Result<()> {
    crate::task_runtime::channel::blocking::require_eof(fd, deadline)
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
