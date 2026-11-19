#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use hf_platform::task_runtime::diag::{DIAG_LIMIT, diag_line_to};
use nix::unistd::pipe;
use std::{
    io::Read,
    os::fd::AsFd,
    time::{Duration, Instant},
};

#[test]
fn a_line_reaches_an_open_pipe() {
    let (read, write) = pipe().unwrap();
    diag_line_to(write.as_fd(), b"harflow-owner: x\n");
    let mut bytes = [0; 64];
    let count = std::fs::File::from(read).read(&mut bytes).unwrap();
    assert_eq!(&bytes[..count], b"harflow-owner: x\n");
}

#[test]
fn a_closed_pipe_is_ignored_without_a_signal() {
    let (read, write) = pipe().unwrap();
    drop(read);
    // SIGPIPE is ignored by the Rust runtime; an EPIPE must not end this process.
    for _ in 0..3 {
        diag_line_to(write.as_fd(), b"harflow-owner: closed\n");
    }
}

#[test]
fn a_full_pipe_never_blocks() {
    let (read, write) = pipe().unwrap();
    // Fill the pipe with non-blocking writes until it refuses more.
    let flags = nix::fcntl::fcntl(&write, nix::fcntl::FcntlArg::F_GETFL).unwrap();
    let nonblocking = nix::fcntl::OFlag::from_bits_truncate(flags) | nix::fcntl::OFlag::O_NONBLOCK;
    nix::fcntl::fcntl(&write, nix::fcntl::FcntlArg::F_SETFL(nonblocking)).unwrap();
    while nix::unistd::write(&write, &[0u8; 4096]).is_ok() {}
    // The pipe is full and blocking again: `diag_line_to` must make it non-blocking itself.
    let blocking = nix::fcntl::OFlag::from_bits_truncate(flags);
    nix::fcntl::fcntl(&write, nix::fcntl::FcntlArg::F_SETFL(blocking)).unwrap();
    let started = Instant::now();
    for _ in 0..100 {
        diag_line_to(write.as_fd(), b"harflow-owner: full\n");
    }
    assert!(started.elapsed() < Duration::from_secs(1));
    let after = nix::fcntl::OFlag::from_bits_truncate(
        nix::fcntl::fcntl(&write, nix::fcntl::FcntlArg::F_GETFL).unwrap(),
    );
    assert!(
        !after.contains(nix::fcntl::OFlag::O_NONBLOCK),
        "the original flags are restored"
    );
    drop(read);
}

#[test]
fn an_oversized_line_is_dropped_whole() {
    let (read, write) = pipe().unwrap();
    diag_line_to(write.as_fd(), &vec![b'x'; DIAG_LIMIT + 1]);
    drop(write);
    let mut bytes = Vec::new();
    std::fs::File::from(read).read_to_end(&mut bytes).unwrap();
    assert!(bytes.is_empty());
}
