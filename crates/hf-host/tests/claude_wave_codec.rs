#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

#[path = "claude_two_tunnel/codec.rs"]
mod codec;

use std::net::{Ipv4Addr, SocketAddr};

const FOUR: &str = "READY\t43127\nSESSION_V2\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\nSESSION_V2\t2\t127.0.0.1\t55002\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_bad_certificate\nSESSION_V2\t3\t127.0.0.1\t55003\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_certificate_unknown\nSESSION_V2\t4\t127.0.0.1\t55004\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\nDONE\t4\n";
const ORIGINAL_TWO: &str = "READY\t43127\nSESSION_V2\t1\t127.0.0.1\t55001\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\nSESSION_V2\t2\t127.0.0.1\t55002\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_bad_certificate\nDONE\t2\n";

fn peers() -> [(usize, SocketAddr); 4] {
    std::array::from_fn(|index| {
        (
            index + 1,
            SocketAddr::from((Ipv4Addr::LOCALHOST, 55001 + index as u16)),
        )
    })
}

#[test]
fn four_sessions_require_complete_unique_ordinal_and_port_binding() {
    let verdict = codec::parse_wave(FOUR.as_bytes()).unwrap();
    assert_eq!(verdict.port, 43127);
    assert_eq!(verdict.sessions.len(), 4);
    assert!(codec::bound_wave_rejection(&verdict, &peers()));
    assert!(codec::parse(FOUR.as_bytes()).is_err());
    let original = codec::parse(ORIGINAL_TWO.as_bytes()).unwrap();
    assert!(codec::bound_rejection(&original, &peers()[..2]));

    let mut mixed_raw = FOUR.as_bytes().to_vec();
    let record = b"SESSION_V2\t4\t127.0.0.1\t55004\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca";
    let replacement =
        b"SESSION_V2\t4\t127.0.0.1\t55004\t0\t0\ttls_error\t1\thandshake\ttls_unclassified";
    let at = mixed_raw
        .windows(record.len())
        .position(|window| window == record)
        .unwrap();
    mixed_raw.splice(at..at + record.len(), replacement.iter().copied());
    let mixed = codec::parse_wave(&mixed_raw).unwrap();
    assert!(!codec::bound_wave_rejection(&mixed, &peers()));
}

#[test]
fn wave_parser_rejects_incomplete_duplicate_foreign_unknown_and_swapped_identity() {
    let missing = FOUR.replace(
        "SESSION_V2\t4\t127.0.0.1\t55004\t1\t0\tcert_alert\t1\thandshake\tcertificate_alert_unknown_ca\n",
        "",
    ).replace("DONE\t4", "DONE\t3");
    let duplicate = FOUR.replace(
        "SESSION_V2\t4\t127.0.0.1\t55004",
        "SESSION_V2\t3\t127.0.0.1\t55004",
    );
    let foreign = FOUR.replace("127.0.0.1\t55004", "127.0.0.1\t55009");
    let unknown = FOUR.replace("127.0.0.1\t55004", "127.0.0.2\t55004");
    let missing = codec::parse_wave(missing.as_bytes()).unwrap();
    assert!(!codec::bound_wave_rejection(&missing, &peers()));
    assert!(codec::parse_wave(duplicate.as_bytes()).is_err());
    let foreign = codec::parse_wave(foreign.as_bytes()).unwrap();
    assert!(!codec::bound_wave_rejection(&foreign, &peers()));
    assert!(codec::parse_wave(unknown.as_bytes()).is_err());
    let swapped = FOUR
        .replace(
            "SESSION_V2\t1\t127.0.0.1\t55001",
            "SESSION_V2\t9\t127.0.0.1\t55001",
        )
        .replace(
            "SESSION_V2\t2\t127.0.0.1\t55002",
            "SESSION_V2\t1\t127.0.0.1\t55002",
        )
        .replace(
            "SESSION_V2\t9\t127.0.0.1\t55001",
            "SESSION_V2\t2\t127.0.0.1\t55001",
        );
    let swapped = codec::parse_wave(swapped.as_bytes()).unwrap();
    assert!(!codec::bound_wave_rejection(&swapped, &peers()));
}
