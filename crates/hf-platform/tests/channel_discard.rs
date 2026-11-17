#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_platform::task_runtime::channel::{Channel, DiscardRead};
use nix::unistd::pipe;
use std::{
    fs::File,
    io::{Read, Write},
    os::fd::OwnedFd,
};

fn file(fd: OwnedFd) -> File {
    File::from(fd)
}

fn fill_pipe(writer: &mut File) -> Vec<u8> {
    let block = [b'f'; 4096];
    let mut filled = Vec::new();
    loop {
        match writer.write(&block) {
            Ok(0) => panic!("pipe writer returned zero"),
            Ok(count) => filled.extend_from_slice(&block[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return filled,
            Err(error) => panic!("pipe fill failed: {error}"),
        }
    }
}

fn drain_available(reader: &mut File) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut block = [0; 4096];
    loop {
        match reader.read(&mut block) {
            Ok(0) => return bytes,
            Ok(count) => bytes.extend_from_slice(&block[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return bytes,
            Err(error) => panic!("pipe drain failed: {error}"),
        }
    }
}

fn framed(body: &[u8]) -> Vec<u8> {
    let mut bytes = (body.len() as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(body);
    bytes
}

#[test]
fn protocol_fault_poison_discards_mutations_but_drains_original_input_to_eof() {
    let (input_read, input_write) = pipe().unwrap();
    let (output_read, output_write) = pipe().unwrap();
    let output = file(output_write);
    let mut fill_handle = output.try_clone().unwrap();
    let mut output_reader = file(output_read);
    let mut channel = Channel::new(file(input_read), output, 32).unwrap();
    let filler = fill_pipe(&mut fill_handle);
    assert!(!filler.is_empty());
    channel.queue(framed(b"pending-control")).unwrap();
    assert!(!channel.flush_step().unwrap());

    let mut input_writer = file(input_write);
    input_writer.write_all(&100_u32.to_be_bytes()).unwrap();
    input_writer.write_all(b"remaining-original-input").unwrap();
    assert!(channel.receive_step().is_err());

    let discarded = channel.discard_step();
    assert_eq!(discarded.buffered_input_bytes, 4);
    assert_eq!(
        discarded.abandoned_output_bytes,
        framed(b"pending-control").len()
    );
    assert!(
        matches!(discarded.read, DiscardRead::Bytes(n) if n == b"remaining-original-input".len())
    );
    assert_eq!(
        channel.queue(framed(b"later")).unwrap_err().kind(),
        std::io::ErrorKind::Other
    );
    assert_eq!(
        channel.flush_step().unwrap_err().kind(),
        std::io::ErrorKind::Other
    );
    assert!(channel.receive_step().is_err());

    assert!(matches!(channel.discard_step().read, DiscardRead::Pending));
    drop(input_writer);
    assert!(matches!(channel.discard_step().read, DiscardRead::Eof));
    assert!(matches!(channel.discard_step().read, DiscardRead::Eof));

    let flags = nix::fcntl::OFlag::from_bits_truncate(
        nix::fcntl::fcntl(&output_reader, nix::fcntl::FcntlArg::F_GETFL).unwrap(),
    );
    nix::fcntl::fcntl(
        &output_reader,
        nix::fcntl::FcntlArg::F_SETFL(flags | nix::fcntl::OFlag::O_NONBLOCK),
    )
    .unwrap();
    assert_eq!(drain_available(&mut output_reader), filler);
}
