#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

#[path = "claude_two_tunnel/codec.rs"]
mod codec;

use std::net::{Ipv4Addr, SocketAddr};

const COMPLETE: &[u8] = b"READY\t43127\nSESSION\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\nSESSION\t2\t127.0.0.1\t55002\t1\t0\tcert_alert\t1\nDONE\t2\n";

fn peers() -> [(usize, SocketAddr); 2] {
    [
        (1, SocketAddr::from((Ipv4Addr::LOCALHOST, 55001))),
        (2, SocketAddr::from((Ipv4Addr::LOCALHOST, 55002))),
    ]
}

#[test]
fn certificate_verdict_requires_complete_unique_session_identity_binding() {
    let verdict = codec::parse(COMPLETE).unwrap();
    assert_eq!(verdict.port, 43127);
    assert_eq!(verdict.sessions.len(), 2);
    assert!(codec::bound_rejection(&verdict, &peers()));
}

#[test]
fn missing_duplicate_or_foreign_session_identity_fails_closed() {
    for raw in [
        b"READY\t43127\nSESSION\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\nSESSION\t1\t127.0.0.1\t55002\t1\t0\tcert_alert\t1\nDONE\t2\n".as_slice(),
        b"READY\t43127\nSESSION\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\nSESSION\t2\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\nDONE\t2\n".as_slice(),
    ] {
        assert!(codec::parse(raw).is_err());
    }
    let missing =
        codec::parse(b"READY\t43127\nSESSION\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\nDONE\t1\n")
            .unwrap();
    assert!(!codec::bound_rejection(&missing, &peers()));
    let foreign = codec::parse(
        b"READY\t43127\nSESSION\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\nSESSION\t2\t127.0.0.1\t55003\t1\t0\tcert_alert\t1\nDONE\t2\n",
    ).unwrap();
    assert!(!codec::bound_rejection(&foreign, &peers()));
}

#[test]
fn eof_timeout_application_data_and_unsettled_sessions_are_not_certificate_rejection() {
    for (status, certificate, application, closed) in [
        ("eof", "0", "0", "1"),
        ("timeout", "0", "0", "1"),
        ("application_data", "0", "1", "1"),
        ("cert_alert", "1", "0", "0"),
    ] {
        let record = format!(
            "READY\t43127\nSESSION\t1\t127.0.0.1\t55001\t{certificate}\t{application}\t{status}\t{closed}\nDONE\t1\n"
        );
        let verdict = codec::parse(record.as_bytes()).unwrap();
        assert!(!codec::bound_rejection(&verdict, &[(1, peers()[0].1)]));
    }
    let inconsistent = b"READY\t43127\nSESSION\t1\t127.0.0.1\t55001\t1\t0\teof\t1\nDONE\t1\n";
    assert!(codec::parse(inconsistent).is_err());
}

#[test]
fn invalid_helper_identity_fields_are_rejected_without_echoing_input() {
    for raw in [
        b"READY\t43127\nSESSION\t0\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\nDONE\t1\n".as_slice(),
        b"READY\t43127\nSESSION\t1\t::1\t55001\t1\t0\tcert_alert\t1\nDONE\t1\n".as_slice(),
        b"READY\t43127\nSESSION\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\nDONE\t1\nEXTRA\n"
            .as_slice(),
    ] {
        let parsed = codec::parse(raw);
        assert!(parsed.is_err());
        assert!(parsed.unwrap_err().len() < 80);
    }
}
