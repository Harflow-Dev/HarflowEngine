#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

#[path = "claude_two_tunnel/codec.rs"]
mod codec;

use std::net::{Ipv4Addr, SocketAddr};

const COMPLETE: &[u8] = b"READY\t43127\nSESSION_V2\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\nSESSION_V2\t2\t127.0.0.1\t55002\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_bad_certificate\nDONE\t2\n";

fn peers() -> [(usize, SocketAddr); 2] {
    [
        (1, SocketAddr::from((Ipv4Addr::LOCALHOST, 55001))),
        (2, SocketAddr::from((Ipv4Addr::LOCALHOST, 55002))),
    ]
}

#[test]
fn versioned_certificate_verdict_requires_complete_unique_identity_binding() {
    let verdict = codec::parse(COMPLETE).unwrap();
    assert_eq!(verdict.port, 43127);
    assert_eq!(verdict.sessions.len(), 2);
    assert!(codec::bound_rejection(&verdict, &peers()));
    assert_eq!(verdict.sessions[0].phase, codec::Phase::Handshake);
    assert_eq!(
        verdict.sessions[0].category,
        codec::Category::CertificateAlertUnknownCa
    );
}

#[test]
fn bound_verdict_rechecks_mutable_session_category_and_phase() {
    let mut verdict = codec::parse(COMPLETE).unwrap();
    verdict.sessions[0].category = codec::Category::PeerAlertInternalError;
    assert!(!codec::bound_rejection(&verdict, &peers()));
    let mut verdict = codec::parse(COMPLETE).unwrap();
    verdict.sessions[0].status = "tls_error";
    assert!(!codec::bound_rejection(&verdict, &peers()));
    let mut verdict = codec::parse(COMPLETE).unwrap();
    verdict.sessions[0].category = codec::Category::LocalCertificateVerificationFailed;
    assert!(!codec::bound_rejection(&verdict, &peers()));
}

#[test]
fn old_schema_and_missing_duplicate_or_foreign_identity_fail_closed() {
    let old = b"READY\t43127\nSESSION\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\nDONE\t1\n";
    assert!(codec::parse(old).is_err());
    for raw in [
        b"READY\t43127\nSESSION_V2\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\nSESSION_V2\t1\t127.0.0.1\t55002\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\nDONE\t2\n".as_slice(),
        b"READY\t43127\nSESSION_V2\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\nSESSION_V2\t2\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\nDONE\t2\n".as_slice(),
    ] {
        assert!(codec::parse(raw).is_err());
    }
    let missing = codec::parse(
        b"READY\t43127\nSESSION_V2\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\nDONE\t1\n",
    )
    .unwrap();
    assert!(!codec::bound_rejection(&missing, &peers()));
    let foreign = codec::parse(
        b"READY\t43127\nSESSION_V2\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\nSESSION_V2\t2\t127.0.0.1\t55003\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\nDONE\t2\n",
    )
    .unwrap();
    assert!(!codec::bound_rejection(&foreign, &peers()));
}

#[test]
fn status_category_phase_and_boolean_pairs_are_strict() {
    let invalid = [
        "SESSION_V2\t1\t127.0.0.1\t55001\t0\t0\tcert_alert\t1\thandshake\tpeer_alert_handshake_failure",
        "SESSION_V2\t1\t127.0.0.1\t55001\t1\t0\ttls_error\t1\thandshake\tcertificate_alert_unknown_ca",
        "SESSION_V2\t1\t127.0.0.1\t55001\t0\t0\tapplication_data\t1\thandshake\tapplication_data",
        "SESSION_V2\t1\t127.0.0.1\t55001\t0\t0\teof\t1\thandshake\tobserved_eof",
        "SESSION_V2\t1\t127.0.0.1\t55001\t0\t0\tapplication_data\t1\tapplication_read\tapplication_data",
        "SESSION_V2\t1\t127.0.0.1\t55001\t0\t0\ttls_error\t0\thandshake\ttls_unclassified",
    ];
    for session in invalid {
        let raw = format!("READY\t43127\n{session}\nDONE\t1\n");
        assert!(codec::parse(raw.as_bytes()).is_err(), "{session}");
    }
}

#[test]
fn all_closed_noncertificate_outcomes_fail_certificate_verdict() {
    for (status, phase, category, application) in [
        (
            "application_data",
            "application_read",
            "application_data",
            "1",
        ),
        ("eof", "application_read", "observed_eof", "0"),
        ("timeout", "handshake", "deadline_timeout", "0"),
        ("tls_error", "handshake", "tls_unclassified", "0"),
        ("tls_error", "handshake", "protocol_unrecognized", "0"),
        (
            "tls_error",
            "handshake",
            "local_certificate_verification_failed",
            "0",
        ),
        ("tls_error", "handshake", "peer_alert_internal_error", "0"),
        ("tls_error", "handshake", "protocol_wrong_version", "0"),
        ("io_error", "handshake", "connection_reset", "0"),
    ] {
        let record = format!(
            "READY\t43127\nSESSION_V2\t1\t127.0.0.1\t55001\t0\t{application}\t{status}\t1\t{phase}\t{category}\nDONE\t1\n"
        );
        let verdict = codec::parse(record.as_bytes()).unwrap();
        assert!(!codec::bound_rejection(&verdict, &[(1, peers()[0].1)]));
    }
}

#[test]
fn unknown_fields_and_sensitive_looking_values_are_not_echoed() {
    for raw in [
        b"READY\t43127\nSESSION_V2\t0\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\nDONE\t1\n".as_slice(),
        b"READY\t43127\nSESSION_V2\t1\t::1\t55001\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\nDONE\t1\n".as_slice(),
        b"READY\t43127\nSESSION_V2\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\thandshake\tsecret-token-like-value\nDONE\t1\n".as_slice(),
        b"READY\t43127\nSESSION_V2\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\tsecret-phase\tcertificate_alert_unknown_ca\nDONE\t1\n".as_slice(),
        b"READY\t43127\nSESSION_V2\t1\t127.0.0.1\t55001\t2\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\nDONE\t1\n".as_slice(),
        b"READY\t43127\nSESSION_V2\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\nDONE\t1\nEXTRA\n".as_slice(),
    ] {
        let parsed = codec::parse(raw);
        assert!(parsed.is_err());
        assert!(parsed.unwrap_err().len() < 80);
    }
}
