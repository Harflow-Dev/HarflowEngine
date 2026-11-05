#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use hf_platform::fixture::{read_frame, require_eof, write_all};
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use std::{
    io::Write,
    net::Shutdown,
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

fn packet() -> Vec<u8> {
    let mut bytes = 62_u32.to_be_bytes().to_vec();
    bytes.extend_from_slice(b"HFB2\x01\x01");
    bytes.extend_from_slice(&42_u32.to_be_bytes());
    bytes.extend_from_slice(&100_u64.to_be_bytes());
    bytes.extend_from_slice(&17_u64.to_be_bytes());
    bytes.extend_from_slice(b"abcdef01-2345-6789-abcd-ef0123456789");
    bytes
}

fn pair() -> (UnixStream, UnixStream) {
    UnixStream::pair().unwrap()
}

#[test]
fn bounded_channel_reads_one_frame_then_requires_clean_eof() {
    let (mut peer, receiver) = pair();
    let expected = packet();
    peer.write_all(&expected).unwrap();
    peer.shutdown(Shutdown::Write).unwrap();
    assert_eq!(
        read_frame(&receiver, Instant::now() + Duration::from_secs(1)).unwrap(),
        expected
    );
    require_eof(&receiver, Instant::now() + Duration::from_secs(1)).unwrap();
}

#[test]
fn channel_refuses_truncation_trailing_input_and_oversized_prefix() {
    let (mut peer, receiver) = pair();
    let mut truncated = packet();
    truncated[3] += 1;
    peer.write_all(&truncated).unwrap();
    peer.shutdown(Shutdown::Write).unwrap();
    assert_eq!(
        read_frame(&receiver, Instant::now() + Duration::from_secs(1))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::UnexpectedEof
    );

    let (mut peer, receiver) = pair();
    let mut trailing = packet();
    trailing.push(0);
    peer.write_all(&trailing).unwrap();
    peer.shutdown(Shutdown::Write).unwrap();
    read_frame(&receiver, Instant::now() + Duration::from_secs(1)).unwrap();
    assert!(require_eof(&receiver, Instant::now() + Duration::from_secs(1)).is_err());

    let (mut peer, receiver) = pair();
    peer.write_all(&24_573_u32.to_be_bytes()).unwrap();
    assert!(read_frame(&receiver, Instant::now() + Duration::from_secs(1)).is_err());
}

#[test]
fn channel_reads_share_one_finite_deadline() {
    let (_peer, receiver) = pair();
    let deadline = Instant::now() + Duration::from_millis(30);
    let error = read_frame(&receiver, deadline).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(Instant::now() < deadline + Duration::from_secs(1));
}

#[test]
fn bounded_writer_delivers_complete_frame_and_refuses_oversized_packet() {
    let (receiver, sender) = pair();
    let expected = packet();
    write_all(&sender, &expected, Instant::now() + Duration::from_secs(1)).unwrap();
    assert_eq!(
        read_frame(&receiver, Instant::now() + Duration::from_secs(1)).unwrap(),
        expected
    );
    assert!(
        write_all(
            &sender,
            &vec![0; 24_577],
            Instant::now() + Duration::from_secs(1)
        )
        .is_err()
    );
}

#[test]
fn expired_channel_operations_refuse_before_changing_descriptor_flags() {
    let (receiver, _peer) = pair();
    let before = OFlag::from_bits_truncate(fcntl(&receiver, FcntlArg::F_GETFL).unwrap());
    let deadline = Instant::now();
    assert_eq!(
        read_frame(&receiver, deadline).unwrap_err().kind(),
        std::io::ErrorKind::TimedOut
    );
    assert_eq!(
        require_eof(&receiver, deadline).unwrap_err().kind(),
        std::io::ErrorKind::TimedOut
    );
    assert_eq!(
        OFlag::from_bits_truncate(fcntl(&receiver, FcntlArg::F_GETFL).unwrap()),
        before
    );

    let (_other, sender) = pair();
    let before = OFlag::from_bits_truncate(fcntl(&sender, FcntlArg::F_GETFL).unwrap());
    assert_eq!(
        write_all(&sender, &packet(), deadline).unwrap_err().kind(),
        std::io::ErrorKind::TimedOut
    );
    assert_eq!(
        OFlag::from_bits_truncate(fcntl(&sender, FcntlArg::F_GETFL).unwrap()),
        before
    );
}
