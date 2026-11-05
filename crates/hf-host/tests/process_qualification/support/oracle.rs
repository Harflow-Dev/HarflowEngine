use super::*;
use hf_domain::process::ProcessIdentity;
use std::{
    io::{self, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
};

pub(in crate::process_qualification) fn run_oracle(
    root: &Path,
    operation: &str,
    cwd: &Path,
) -> String {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/readonly_oracle.py");
    let args = vec![
        OsString::from("-I"),
        OsString::from("-S"),
        script.into_os_string(),
        root.as_os_str().to_owned(),
        operation.into(),
    ];
    let mut oracle = ChildGuard::spawn(PathBuf::from(PYTHON), args, cwd.to_owned());
    oracle.0.close_stdin();
    oracle.finish();
    drain_capture(&mut oracle);
    String::from_utf8(oracle.0.capture().stdout.bytes.clone()).expect("oracle UTF-8")
}
fn drain_capture(oracle: &mut ChildGuard) {
    let end = Instant::now() + Duration::from_secs(1);
    loop {
        let capture = oracle.0.capture();
        if capture.stdout.eof && capture.stderr.eof {
            assert!(capture.stdout.error.is_none() && capture.stderr.error.is_none());
            assert!(!capture.stdout.overflow && !capture.stderr.overflow);
            return;
        }
        assert!(Instant::now() < end, "bounded oracle stream drain expired");
        oracle
            .0
            .pump(end, 8192)
            .expect("drain bounded oracle capture");
    }
}
pub(in crate::process_qualification) fn oracle_process(json: &str, key: &str) -> ProcessIdentity {
    let marker = format!("\"{key}\": \"");
    let encoded = json
        .split(&marker)
        .nth(1)
        .expect("oracle field")
        .split('"')
        .next()
        .expect("field terminator");
    let mut fields = encoded.split(':');
    ProcessIdentity::new(
        fields.next().expect("PID").parse().expect("integer PID"),
        fields
            .next()
            .expect("start seconds")
            .parse()
            .expect("integer start seconds"),
        fields
            .next()
            .expect("start micros")
            .parse()
            .expect("integer start micros"),
        fields.next().expect("boot UUID").to_owned(),
    )
    .expect("raw process identity")
}
pub(in crate::process_qualification) fn oracle_generation(json: &str) -> Id {
    let encoded = json
        .split("\"generation\": \"")
        .nth(1)
        .expect("generation field")
        .split('"')
        .next()
        .expect("generation terminator");
    Id::new(encoded.to_owned()).expect("valid stored generation")
}
pub(in crate::process_qualification) fn oracle_probe_pid(json: &str) -> Option<u32> {
    let marker = "\"probe_pid\": ";
    let value = json
        .split(marker)
        .nth(1)
        .expect("probe field")
        .split(',')
        .next()
        .expect("probe value")
        .trim();
    if value == "null" {
        None
    } else {
        Some(value.trim_matches('"').parse().expect("probe PID integer"))
    }
}
pub(in crate::process_qualification) fn accept_until(
    listener: &UnixListener,
    end: Instant,
) -> io::Result<UnixStream> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < end => {
                std::thread::yield_now()
            }
            Err(error) => return Err(error),
        }
    }
}
pub(in crate::process_qualification) fn read_until(
    stream: &mut UnixStream,
    bytes: &mut [u8],
    end: Instant,
) -> io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        match stream.read(&mut bytes[offset..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "short gate packet",
                ));
            }
            Ok(count) => offset += count,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < end => {
                std::thread::yield_now()
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
pub(in crate::process_qualification) fn write_until(
    stream: &mut UnixStream,
    bytes: &[u8],
    end: Instant,
) -> io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        match stream.write(&bytes[offset..]) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "short gate echo")),
            Ok(count) => offset += count,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < end => {
                std::thread::yield_now()
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
pub(in crate::process_qualification) fn expect_eof(
    stream: &mut UnixStream,
    end: Instant,
) -> io::Result<()> {
    let mut byte = [0_u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => return Ok(()),
            Ok(_) => return Err(io::Error::other("unexpected gate trailing byte")),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < end => {
                std::thread::yield_now()
            }
            Err(error) => return Err(error),
        }
    }
}
