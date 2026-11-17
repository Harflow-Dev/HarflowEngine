//! Owned, nonblocking private length-framed channels. One bounded operation per tick.
pub(crate) mod blocking;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use std::{
    fs::File,
    io::{self, Read, Write},
};
pub struct Channel {
    input: File,
    output: File,
    cap: usize,
    prefix: [u8; 4],
    prefix_at: usize,
    body: Vec<u8>,
    body_at: usize,
    pending: Vec<u8>,
    sent: usize,
    poisoned: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Receive {
    Pending,
    Eof,
    Frame(Vec<u8>),
}
/// One actual storage-free drainage read. Errors never manufacture EOF.
#[derive(Debug)]
pub enum DiscardRead {
    Bytes(usize),
    Pending,
    Eof,
    Error(io::Error),
}
#[derive(Debug)]
pub struct DiscardStep {
    /// Already-read protocol prefix/body bytes abandoned by this call.
    pub buffered_input_bytes: usize,
    /// Unsent protocol bytes abandoned; previously sent bytes are never replayed.
    pub abandoned_output_bytes: usize,
    pub read: DiscardRead,
}
fn refused() -> io::Error {
    io::Error::other("private channel refused")
}
impl Channel {
    /// Ownership of both endpoints is transferred. The cap includes the four-byte prefix.
    pub fn new(input: File, output: File, cap: usize) -> io::Result<Self> {
        if !(5..=65536).contains(&cap) {
            return Err(refused());
        }
        for fd in [&input, &output] {
            let flags = OFlag::from_bits_truncate(fcntl(fd, FcntlArg::F_GETFL)?);
            fcntl(fd, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
        }
        Ok(Self {
            input,
            output,
            cap,
            prefix: [0; 4],
            prefix_at: 0,
            body: Vec::new(),
            body_at: 0,
            pending: Vec::new(),
            sent: 0,
            poisoned: false,
        })
    }
    /// Takes a complete already-framed packet; never accepts a second pending frame.
    pub fn queue(&mut self, frame: Vec<u8>) -> io::Result<()> {
        if self.poisoned || !self.pending.is_empty() || frame.len() < 5 || frame.len() > self.cap {
            return Err(refused());
        }
        let size = u32::from_be_bytes(frame[..4].try_into().map_err(|_| refused())?) as usize;
        if size != frame.len() - 4 {
            return Err(refused());
        }
        self.pending = frame;
        self.sent = 0;
        Ok(())
    }
    pub fn flush_step(&mut self) -> io::Result<bool> {
        if self.poisoned {
            return Err(refused());
        }
        if self.pending.is_empty() {
            return Ok(true);
        }
        match self.output.write(&self.pending[self.sent..]) {
            Ok(0) => {
                self.poisoned = true;
                Err(io::Error::new(io::ErrorKind::WriteZero, "private channel"))
            }
            Ok(count) => {
                self.sent += count;
                if self.sent == self.pending.len() {
                    self.pending.clear();
                    self.sent = 0;
                }
                Ok(self.pending.is_empty())
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(false)
            }
            Err(e) => {
                self.poisoned = true;
                Err(e)
            }
        }
    }
    pub fn receive_step(&mut self) -> io::Result<Receive> {
        if self.poisoned {
            return Err(refused());
        }
        let result = self.receive_inner();
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    /// Irreversibly abandon protocol mutation and drain the original input only.
    /// Available after any protocol fault, I/O failure or EOF. Each call makes at
    /// most one nonblocking read of 4096 bytes and retains both original handles.
    pub fn discard_step(&mut self) -> DiscardStep {
        self.poisoned = true;
        let buffered_input_bytes = self.prefix_at + self.body_at;
        let abandoned_output_bytes = self.pending.len().saturating_sub(self.sent);
        self.prefix = [0; 4];
        self.prefix_at = 0;
        self.body.clear();
        self.body_at = 0;
        self.pending.clear();
        self.sent = 0;
        let mut bytes = [0; 4096];
        let read = match self.input.read(&mut bytes) {
            Ok(0) => DiscardRead::Eof,
            Ok(count) => DiscardRead::Bytes(count),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                DiscardRead::Pending
            }
            Err(error) => DiscardRead::Error(error),
        };
        DiscardStep {
            buffered_input_bytes,
            abandoned_output_bytes,
            read,
        }
    }
    fn receive_inner(&mut self) -> io::Result<Receive> {
        let prefix = self.prefix_at < 4;
        let target = if prefix {
            &mut self.prefix[self.prefix_at..]
        } else {
            &mut self.body[self.body_at..]
        };
        match self.input.read(target) {
            Ok(0) if prefix && self.prefix_at == 0 => return Ok(Receive::Eof),
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "private channel truncated",
                ));
            }
            Ok(n) if prefix => self.prefix_at += n,
            Ok(n) => self.body_at += n,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                return Ok(Receive::Pending);
            }
            Err(e) => return Err(e),
        }
        if self.prefix_at == 4 && self.body.is_empty() {
            let size = u32::from_be_bytes(self.prefix) as usize;
            if size == 0 || size > self.cap - 4 {
                return Err(refused());
            }
            self.body.resize(size, 0);
        }
        if self.body.is_empty() || self.body_at < self.body.len() {
            return Ok(Receive::Pending);
        }
        let mut frame = self.prefix.to_vec();
        frame.append(&mut self.body);
        self.prefix_at = 0;
        self.body_at = 0;
        Ok(Receive::Frame(frame))
    }
}
