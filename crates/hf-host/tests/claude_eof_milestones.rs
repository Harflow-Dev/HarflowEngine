#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_domain::process::ProcessExit;
use hf_platform::{
    process::{MacIdentity, RetainedOwnedChild, SpawnSpec},
    qualification::FixtureRoot,
};
use std::{
    fs,
    time::{Duration, Instant},
};

#[path = "claude_live/actors.rs"]
#[allow(dead_code)]
mod actors;
#[path = "claude_two_tunnel/codec.rs"]
#[allow(dead_code)]
mod codec;
#[path = "claude_two_tunnel/milestone_codec.rs"]
#[allow(dead_code)]
mod milestone_codec;

const PYTHON: &str = "/Library/Developer/CommandLineTools/usr/bin/python3";
const DRIVER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/claude_eof_milestones/driver.py"
);
const CONTROL: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/claude_eof_milestones/observer_controls.py"
);

#[test]
fn native_observer_ignores_payload_and_fails_closed() {
    let root = FixtureRoot::create().unwrap();
    let output = collect_python(CONTROL, &root, Duration::from_secs(5));
    assert_eq!(output, b"OBSERVER_CONTROLS\tpass\n");
}

#[test]
fn actual_four_peer_milestones_bind_to_closed_sessions() {
    let root = FixtureRoot::create().unwrap();
    let output = collect_python(DRIVER, &root, Duration::from_secs(32));
    let text = std::str::from_utf8(&output).unwrap();
    let mut lines = text.lines();
    let peers = lines
        .next()
        .unwrap()
        .strip_prefix("PEERS\t")
        .unwrap()
        .split('\t')
        .map(|port| port.parse::<u16>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(peers.len(), 4);
    assert!(peers.iter().all(|port| *port != 0));
    let metadata = format!("{}\n", lines.collect::<Vec<_>>().join("\n"));
    let diagnostic = milestone_codec::parse(metadata.as_bytes()).unwrap();
    assert_eq!(diagnostic.verdict.sessions.len(), 4);
    eprintln!("phase=local-eof-milestones verdict={diagnostic:?}");
    for (index, port) in peers.iter().enumerate() {
        let session = diagnostic
            .verdict
            .sessions
            .iter()
            .find(|s| s.ordinal == index + 1)
            .unwrap();
        let row = diagnostic
            .milestones
            .iter()
            .find(|m| m.ordinal == index + 1)
            .unwrap();
        assert_eq!(session.peer_port, *port);
        assert_eq!(row.peer_port, *port);
        assert!(session.closed);
        match index {
            0 => {
                assert!(row.flags.read_client_hello && row.flags.read_client_key_exchange);
                assert!(row.flags.read_finished && row.flags.write_finished);
                assert!(row.flags.write_server_hello && row.flags.write_certificate);
                assert!(row.flags.write_server_key_exchange && row.flags.write_server_hello_done);
                assert!(row.flags.read_ccs && row.flags.write_ccs);
                assert!(matches!(&row.sni, milestone_codec::Sni::Matched));
                assert!(session.application_data);
            }
            1 => {
                require_first_flight(row);
                assert_eq!(session.phase, codec::Phase::Handshake);
                assert_eq!(session.category, codec::Category::CertificateAlertUnknownCa);
                assert!(row.flags.read_client_hello && row.flags.read_alert);
                assert!(!row.flags.read_client_key_exchange);
                assert!(matches!(&row.sni, milestone_codec::Sni::Matched));
                assert!(session.certificate_rejected);
            }
            2 => {
                require_first_flight(row);
                assert_eq!(session.phase, codec::Phase::Handshake);
                assert_eq!(session.category, codec::Category::TransportEof);
                assert!(row.flags.read_client_hello);
                assert!(!row.flags.read_client_key_exchange && !row.flags.read_finished);
                assert!(matches!(&row.sni, milestone_codec::Sni::Matched));
                assert!(!session.certificate_rejected);
            }
            _ => {
                assert_eq!(session.phase, codec::Phase::Handshake);
                assert_eq!(session.category, codec::Category::TransportEof);
                assert!(!row.flags.read_client_hello);
                assert!(!row.flags.read_client_key_exchange && !row.flags.read_finished);
                assert!(matches!(&row.sni, milestone_codec::Sni::Unseen));
                assert!(!session.certificate_rejected);
            }
        }
    }
}

fn require_first_flight(row: &milestone_codec::Milestone) {
    assert!(row.flags.write_server_hello && row.flags.write_certificate);
    assert!(row.flags.write_server_key_exchange && row.flags.write_server_hello_done);
}

#[test]
fn milestone_parser_rejects_incomplete_duplicate_and_sensitive_rows() {
    let common = b"READY\t40000\nSESSION_V2\t1\t127.0.0.1\t40001\t0\t0\ttls_error\t1\thandshake\ttransport_eof\nDONE\t1\n";
    let valid = [
        common.as_slice(),
        b"TLS_MILESTONE_V1\t1\t127.0.0.1\t40001\t0\tunseen\n",
    ]
    .concat();
    assert!(milestone_codec::parse(&valid).is_ok());
    for suffix in [
        "",
        "TLS_MILESTONE_V1\t1\t127.0.0.1\t40001\t4096\tunseen\n",
        "TLS_MILESTONE_V1\t1\t127.0.0.1\t40001\t0\tsecret.example\n",
        "TLS_MILESTONE_V1\t1\t127.0.0.2\t40001\t0\tunseen\n",
        "TLS_MILESTONE_V1\t1\t127.0.0.1\t40002\t0\tunseen\n",
        "TLS_MILESTONE_V1\t1\t127.0.0.1\t40001\t0\tunseen\nTLS_MILESTONE_V1\t1\t127.0.0.1\t40001\t0\tunseen\n",
        "TLS_MILESTONE_V1\t2\t127.0.0.1\t40001\t0\tunseen\n",
    ] {
        let mut raw = common.to_vec();
        raw.extend_from_slice(suffix.as_bytes());
        let error = milestone_codec::parse(&raw).unwrap_err();
        assert!(!error.contains("secret.example"));
    }
}

fn collect_python(script: &str, root: &FixtureRoot, budget: Duration) -> Vec<u8> {
    let directory = root.outside().join("eof-milestones");
    fs::create_dir(&directory).unwrap();
    let spec = SpawnSpec {
        executable: fs::canonicalize(PYTHON).unwrap(),
        cwd: std::env::current_dir().unwrap(),
        args: vec![
            "-I".into(),
            "-S".into(),
            script.into(),
            directory.clone().into_os_string(),
        ],
        environment: vec![
            ("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into()),
            ("HOME".into(), directory.into_os_string()),
            ("LANG".into(), "C".into()),
            ("LC_ALL".into(), "C".into()),
            ("PYTHONDONTWRITEBYTECODE".into(), "1".into()),
        ],
        capture_limit: 4096,
    };
    let child =
        RetainedOwnedChild::spawn(spec, &MacIdentity, Instant::now() + Duration::from_secs(3))
            .unwrap();
    let result = actors::Original::new(child).collect(Instant::now() + budget);
    assert!(
        result.complete && result.exit == ProcessExit::Code(0),
        "closed helper failed: exit={:?}, complete={}, static_driver_failure={}",
        result.exit,
        result.complete,
        result.stderr == b"DRIVER_FAILURE\n"
    );
    assert!(result.stderr.is_empty());
    result.stdout
}
