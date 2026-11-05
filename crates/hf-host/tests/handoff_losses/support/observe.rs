use super::{Owned, PYTHON, deadline, drain_capture, id};
use hf_domain::fixture::Stage;
use hf_platform::fixture::gate_packet;
use hf_platform::process::WaitOutcome;
use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    time::Instant,
};

pub fn gate_peer(
    listener: &UnixListener,
    stage: Stage,
    operation: &str,
    nonce: &str,
) -> UnixStream {
    let end = deadline(7);
    let (mut peer, _) = loop {
        match listener.accept() {
            Ok(pair) => break pair,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < end =>
            {
                std::thread::yield_now()
            }
            Err(error) => panic!("gate accept failed: {error}"),
        }
    };
    peer.set_nonblocking(true).expect("nonblocking gate peer");
    let expected = gate_packet(stage, &id(operation), &id(nonce));
    let mut received = vec![0; expected.len()];
    read_until(&mut peer, &mut received, deadline(1)).expect("read bounded gate packet");
    assert_eq!(received, expected);
    peer
}

pub fn echo_gate(peer: &mut UnixStream, stage: Stage, operation: &str, nonce: &str) {
    let packet = gate_packet(stage, &id(operation), &id(nonce));
    let end = deadline(1);
    let mut written = 0;
    while written < packet.len() {
        match peer.write(&packet[written..]) {
            Ok(0) => panic!("short gate echo"),
            Ok(count) => written += count,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < end =>
            {
                std::thread::yield_now()
            }
            Err(error) => panic!("gate echo failed: {error}"),
        }
    }
    peer.shutdown(std::net::Shutdown::Write)
        .expect("gate response EOF");
}

fn read_until(peer: &mut UnixStream, bytes: &mut [u8], until: Instant) -> std::io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        match peer.read(&mut bytes[offset..]) {
            Ok(0) => return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof)),
            Ok(count) => offset += count,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < until =>
            {
                std::thread::yield_now()
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub fn oracle(root: &Path, operation: &str, cwd: &Path) -> String {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/readonly_oracle.py");
    let mut child = Owned::spawn(
        PathBuf::from(PYTHON),
        vec![
            "-I".into(),
            "-S".into(),
            script.into_os_string(),
            root.as_os_str().into(),
            operation.into(),
        ],
        cwd.to_path_buf(),
    );
    child.0.close_stdin();
    let until = deadline(2);
    assert!(
        matches!(
            child.0.wait_until(until, || false),
            Ok(WaitOutcome::Exited(hf_domain::process::ProcessExit::Code(
                0
            )))
        ),
        "read-only oracle failed"
    );
    drain_capture(&mut child, until).expect("bounded oracle output drain");
    assert!(
        child.0.capture().stdout.eof && child.0.capture().stderr.eof,
        "oracle output streams must reach EOF"
    );
    assert!(
        !child.0.capture().stdout.overflow,
        "read-only oracle output truncated"
    );
    String::from_utf8(child.0.capture().stdout.bytes.clone())
        .expect("oracle UTF-8")
        .trim()
        .to_owned()
}

pub fn number(json: &str, key: &str) -> Option<u64> {
    json.split(&format!("\"{key}\": "))
        .nth(1)?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

pub fn assert_empty(path: &Path) {
    assert_eq!(
        std::fs::read_to_string(path)
            .expect("effect ledger")
            .lines()
            .count(),
        0
    );
}
