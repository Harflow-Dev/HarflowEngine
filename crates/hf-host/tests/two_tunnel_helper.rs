#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used)]

use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::thread;

const CONTROLS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/two_tunnel_helper/helper_controls.py"
);
const HELPER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/claude_two_tunnel/fake_tls.py"
);

fn drain<R: Read + Send + 'static>(mut reader: R) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[test]
fn actual_two_session_helper_distinguishes_tls_outcomes() {
    let mut child = Command::new("/usr/bin/python3")
        .arg("-I")
        .arg("-S")
        .arg(CONTROLS)
        .arg(HELPER)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start offline TLS helper controls");
    let child_stdout = child.stdout.take().expect("control stdout");
    let child_stderr = child.stderr.take().expect("control stderr");
    let stdout = thread::spawn(move || drain(child_stdout));
    let stderr = thread::spawn(move || drain(child_stderr));
    let status = child.wait().expect("wait for controls process");
    let out = stdout
        .join()
        .expect("join control stdout drain")
        .expect("read control stdout");
    let err = stderr
        .join()
        .expect("join control stderr drain")
        .expect("read control stderr");
    assert!(status.success(), "offline controls failed: exit={status}");
    assert!(err.is_empty(), "offline controls wrote unexpected stderr");
    let text = std::str::from_utf8(&out).expect("static control output");
    for result in [
        "CONTROL\ttrusted_application_data\tpass",
        "CONTROL\tdefault_certificate_rejection\tpass",
        "CONTROL\traw_tcp_eof_not_certificate_rejection\tpass",
        "CONTROL\tidle_timeout_not_certificate_rejection\tpass",
        "CONTROL\tcontroller_stop_not_certificate_rejection\tpass",
        "CONTROL\tpeer_identity_binding\tpass",
    ] {
        assert!(
            text.lines().any(|line| line == result),
            "missing control result: {result}"
        );
    }
}
