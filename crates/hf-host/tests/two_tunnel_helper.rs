#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used)]

use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::thread;

#[path = "claude_two_tunnel/codec.rs"]
mod codec;

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

fn assert_actual_metadata(text: &str) {
    let mut actual = Vec::new();
    let mut collecting = false;
    let mut completed = 0;
    let mut observed_malformed = false;
    let mut observed_fatal_alert_a = false;
    let mut observed_fatal_alert_b = false;
    for line in text.lines() {
        if line.starts_with("OBSERVED\tmalformed_tls\thandshake\t") {
            observed_malformed = matches!(
                line,
                "OBSERVED\tmalformed_tls\thandshake\tprotocol_unrecognized"
            );
        }
        observed_fatal_alert_a |= matches!(
            line,
            "OBSERVED\tfatal_alert_bytes_a\thandshake\tprotocol_unrecognized"
        );
        observed_fatal_alert_b |= matches!(
            line,
            "OBSERVED\tfatal_alert_bytes_b\thandshake\tprotocol_unrecognized"
        );
        if line.starts_with("READY\t") {
            actual.clear();
            collecting = true;
        }
        if collecting {
            actual.extend_from_slice(line.as_bytes());
            actual.push(b'\n');
            if line.starts_with("DONE\t") {
                assert_roundtrip(&actual);
                completed += 1;
                collecting = false;
            }
        }
    }
    assert!(!collecting, "actual helper metadata did not complete");
    assert!(
        completed >= 1,
        "no complete actual helper transcript was parsed"
    );
    assert!(observed_malformed, "malformed TLS outcome was not observed");
    assert!(text.lines().any(|line| {
        matches!(
            line,
            "OBSERVED\traw_eof\thandshake\ttransport_eof"
                | "OBSERVED\traw_eof\thandshake\ttls_unclassified"
        )
    }));
    assert!(
        text.lines()
            .any(|line| line == "OBSERVED\tidle_timeout\thandshake\tdeadline_timeout")
    );
    assert!(
        observed_fatal_alert_a,
        "first raw prehello alert was not observed"
    );
    assert!(
        observed_fatal_alert_b,
        "second raw prehello alert was not observed"
    );
}

fn assert_roundtrip(actual: &[u8]) {
    let verdict = codec::parse(actual).expect("actual helper metadata codec roundtrip");
    assert_ne!(verdict.port, 0);
    assert_eq!(verdict.sessions.len(), 2);
    assert!(
        verdict
            .sessions
            .iter()
            .all(|s| s.category.status() == s.status)
    );
    assert!(verdict.sessions.iter().any(|s| {
        s.category == codec::Category::ApplicationData && s.phase == codec::Phase::ApplicationRead
    }));
    assert!(verdict.sessions.iter().any(|s| {
        matches!(
            s.category,
            codec::Category::CertificateAlertUnknownCa
                | codec::Category::CertificateAlertBadCertificate
                | codec::Category::CertificateAlertCertificateUnknown
        ) && s.phase == codec::Phase::Handshake
    }));
    let peers = verdict
        .sessions
        .iter()
        .map(|s| {
            (
                s.ordinal,
                std::net::SocketAddr::from(([127, 0, 0, 1], s.peer_port)),
            )
        })
        .collect::<Vec<_>>();
    assert!(!codec::bound_rejection(&verdict, &peers));
}

#[test]
fn actual_two_session_helper_distinguishes_tls_outcomes() {
    let mut child = Command::new("/Library/Developer/CommandLineTools/usr/bin/python3")
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
    assert_actual_metadata(text);
    for result in [
        "CONTROL\ttrusted_application_data\tpass",
        "CONTROL\tdefault_certificate_rejection\tpass",
        "CONTROL\traw_tcp_eof_not_certificate_rejection\tpass",
        "CONTROL\tidle_timeout_not_certificate_rejection\tpass",
        "CONTROL\tcontroller_stop_not_certificate_rejection\tpass",
        "CONTROL\tpeer_identity_binding\tpass",
        "CONTROL\tmalformed_tls_protocol_unrecognized\tpass",
        "CONTROL\tprehello_protocol_unrecognized\tpass",
    ] {
        assert!(
            text.lines().any(|line| line == result),
            "missing control result: {result}"
        );
    }
}
