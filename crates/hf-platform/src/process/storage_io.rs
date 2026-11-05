//! Incremental private writer transport; owns pipes, never child wait authority.
use nix::poll::{PollFd, PollFlags, poll};
use std::{
    fs::File,
    io::{self, Read, Write},
    os::fd::AsFd,
    time::Instant,
};
const BODY: usize = 24572;
const HEADER: usize = 82;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorageStderr {
    pub retained: Vec<u8>,
    pub discarded: u64,
    pub eof: bool,
    pub unavailable: bool,
}
pub struct PrivateStorageIo {
    input: Option<File>,
    output: File,
    error: File,
    tx: Option<Vec<u8>>,
    sent: usize,
    tx_abandoned: bool,
    rx_invalid: bool,
    rx: Vec<u8>,
    expected: Option<usize>,
    frame: Option<Vec<u8>>,
    eof: bool,
    failed: bool,
    stderr: StorageStderr,
    stderr_read: usize,
}
impl PrivateStorageIo {
    pub(super) fn new(input: File, output: File, error: File) -> Self {
        Self {
            input: Some(input),
            output,
            error,
            tx: None,
            sent: 0,
            tx_abandoned: false,
            rx_invalid: false,
            rx: Vec::with_capacity(4),
            expected: None,
            frame: None,
            eof: false,
            failed: false,
            stderr: StorageStderr::default(),
            stderr_read: 0,
        }
    }
    /// Takes ownership of one complete codec frame, including its length prefix.
    pub fn queue_frame(&mut self, frame: Vec<u8>) -> io::Result<()> {
        if self.input.is_none()
            || self.tx.is_some()
            || self.failed
            || frame.len() < HEADER
            || frame.len() > BODY + 4
        {
            return Err(io::Error::other("storage frame admission refused"));
        }
        let body = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
        if body != frame.len() - 4 {
            return Err(io::Error::other("storage frame prefix mismatch"));
        }
        self.tx = Some(frame);
        self.sent = 0;
        Ok(())
    }
    pub fn close_input(&mut self) {
        self.tx_abandoned |= self.tx.is_some();
        self.input = None;
    }
    pub fn send_complete(&self) -> bool {
        self.tx.is_none() && !self.failed && !self.tx_abandoned
    }
    pub fn tx_abandoned(&self) -> bool {
        self.tx_abandoned
    }
    pub fn sent_offset(&self) -> usize {
        self.sent
    }
    /// Physical pipe EOF only. This does not acknowledge any storage operation.
    pub fn channels_complete(&self) -> bool {
        self.eof && self.stderr.eof
    }
    pub fn take_frame(&mut self) -> Option<Vec<u8>> {
        self.frame.take()
    }
    pub fn stderr(&self) -> &StorageStderr {
        &self.stderr
    }
    pub fn is_complete(&self) -> bool {
        self.eof
            && self.stderr.eof
            && self.tx.is_none()
            && self.rx.is_empty()
            && self.frame.is_none()
            && !self.failed
    }
    pub fn step(&mut self, deadline: Instant) -> io::Result<()> {
        if Instant::now() >= deadline {
            return Ok(());
        }
        let mut descriptors = Vec::with_capacity(3);
        if !self.eof && self.frame.is_none() {
            descriptors.push(PollFd::new(self.output.as_fd(), PollFlags::POLLIN));
        }
        if !self.stderr.eof && !self.stderr.unavailable {
            descriptors.push(PollFd::new(self.error.as_fd(), PollFlags::POLLIN));
        }
        if let Some(input) = &self.input
            && self.tx.is_some()
        {
            descriptors.push(PollFd::new(input.as_fd(), PollFlags::POLLOUT));
        }
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(20) as u16;
        match poll(&mut descriptors, timeout) {
            Ok(_) => (),
            Err(nix::errno::Errno::EINTR) => return Ok(()),
            Err(error) => return self.fail(error.into()),
        }
        drop(descriptors);
        let sent = self.send();
        let received = self.receive();
        let drained = self.drain_error();
        sent.and(received).and(drained)
    }
    fn fail<T>(&mut self, error: io::Error) -> io::Result<T> {
        self.failed = true;
        self.close_input();
        Err(error)
    }
    fn send(&mut self) -> io::Result<()> {
        let (Some(input), Some(tx)) = (&mut self.input, &self.tx) else {
            return Ok(());
        };
        match input.write(&tx[self.sent..tx.len().min(self.sent + 4096)]) {
            Ok(0) => self.fail(io::Error::new(
                io::ErrorKind::WriteZero,
                "storage input stalled",
            )),
            Ok(count) => {
                self.sent += count;
                if self.sent == tx.len() {
                    self.tx = None;
                }
                Ok(())
            }
            Err(error) if transient(&error) => Ok(()),
            Err(error) => self.fail(error),
        }
    }
    fn receive(&mut self) -> io::Result<()> {
        if self.eof || self.frame.is_some() {
            return Ok(());
        }
        let target = self.expected.unwrap_or(4);
        let mut buffer = [0_u8; 4096];
        let limit = if self.rx_invalid {
            buffer.len()
        } else {
            buffer.len().min(target - self.rx.len())
        };
        match self.output.read(&mut buffer[..limit]) {
            Ok(0) => {
                self.eof = true;
                if !self.rx.is_empty() {
                    return self.fail(io::Error::other("truncated storage frame"));
                }
            }
            Ok(count) => {
                if !self.rx_invalid {
                    self.rx.extend_from_slice(&buffer[..count]);
                    self.decode()?;
                }
            }
            Err(error) if transient(&error) => (),
            Err(error) => return self.fail(error),
        }
        Ok(())
    }
    fn decode(&mut self) -> io::Result<()> {
        if self.expected.is_none() && self.rx.len() == 4 {
            let length =
                u32::from_be_bytes([self.rx[0], self.rx[1], self.rx[2], self.rx[3]]) as usize;
            if !(HEADER - 4..=BODY).contains(&length) {
                self.rx_invalid = true;
                return self.fail(io::Error::other("oversized storage frame"));
            }
            self.expected = Some(length + 4);
            self.rx.reserve(length);
        }
        if self.expected == Some(self.rx.len()) {
            self.frame = Some(std::mem::replace(&mut self.rx, Vec::with_capacity(4)));
            self.expected = None;
        }
        Ok(())
    }
    fn drain_error(&mut self) -> io::Result<()> {
        if self.stderr.eof || self.stderr.unavailable {
            return Ok(());
        }
        let mut buffer = [0_u8; 4096];
        let limit = buffer.len().min(1048576 - self.stderr_read);
        if limit == 0 {
            self.stderr.unavailable = true;
            return self.fail(io::Error::other("storage stderr read bound reached"));
        }
        match self.error.read(&mut buffer[..limit]) {
            Ok(0) => self.stderr.eof = true,
            Ok(count) => {
                self.stderr_read += count;
                let keep = count.min(4096 - self.stderr.retained.len());
                self.stderr.retained.extend_from_slice(&buffer[..keep]);
                self.stderr.discarded += (count - keep) as u64;
            }
            Err(error) if transient(&error) => (),
            Err(error) => {
                self.stderr.unavailable = true;
                return self.fail(error);
            }
        }
        Ok(())
    }
}
fn transient(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}
