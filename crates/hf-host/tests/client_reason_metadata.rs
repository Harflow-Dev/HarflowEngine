#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_domain::process::ProcessIdentity;

#[path = "claude_two_tunnel/client_reason_metadata.rs"]
mod client_reason_metadata;

const NONCE: &str = "0123456789abcdef0123456789abcdef";

fn identity(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 1, 0, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}

fn prefix(nonce: &str, pid: u32) -> Vec<u8> {
    format!("CR1\t{nonce}\t{pid}\n").into_bytes()
}

fn complete(nonce: &str, pid: u32) -> Vec<u8> {
    format!(
        "CR1\t{nonce}\t{pid}\nCE1\t{nonce}\t{pid}\t1\tverify\tissuer\nCX1\t{nonce}\t{pid}\t1\t1\t0\t1\n"
    )
    .into_bytes()
}

#[test]
fn accepts_bound_complete_and_incomplete_prefixes() {
    let process = identity(42);
    let partial = client_reason_metadata::parse(&prefix(NONCE, 42), NONCE, &process).unwrap();
    assert!(!partial.complete && !partial.healthy);
    assert!(partial.records.is_empty());

    let observation = client_reason_metadata::parse(&complete(NONCE, 42), NONCE, &process).unwrap();
    assert!(observation.complete && observation.healthy);
    assert_eq!(observation.calls, 1);
    assert_eq!(observation.records.len(), 1);
}

#[test]
fn rejects_malformed_or_unbound_metadata_with_static_error() {
    let process = identity(42);
    let valid = complete(NONCE, 42);
    let rows = [
        b"CR2\t0123456789abcdef0123456789abcdef\n".to_vec(),
        valid
            .iter()
            .map(|b| b.to_ascii_uppercase())
            .collect::<Vec<_>>(),
        [prefix(NONCE, 42), prefix(NONCE, 42)].concat(),
        [prefix(NONCE, 42), b"CE1\tbad\n".to_vec()].concat(),
        [prefix(NONCE, 42), b"NOISE\tsecret\n".to_vec()].concat(),
        [
            prefix(NONCE, 42),
            b"CE1\t0123456789abcdef0123456789abcdef\t42\t1\tprivate-text\tmissing\n".to_vec(),
        ]
        .concat(),
        [
            prefix(NONCE, 42),
            b"CX1\t0123456789abcdef0123456789abcdef\t42\t4\t0\t1\t1\n".to_vec(),
        ]
        .concat(),
        vec![b'x'; 513],
    ];
    for bytes in rows {
        let error = client_reason_metadata::parse(&bytes, NONCE, &process).unwrap_err();
        assert_eq!(error, "unavailable");
        assert!(!error.contains("private-text"));
        assert!(!error.contains(NONCE));
    }
    assert_eq!(
        client_reason_metadata::parse(&valid, "short", &process),
        Err("unavailable")
    );
    assert_eq!(
        client_reason_metadata::parse(&valid, NONCE, &identity(43)),
        Err("unavailable")
    );
}

#[test]
fn enforces_order_uniqueness_counts_and_end_consistency() {
    let pid = 42;
    let start = prefix(NONCE, pid);
    let ce1 = format!("CE1\t{NONCE}\t{pid}\t1\tverify\tissuer\n");
    let ce2 = format!("CE1\t{NONCE}\t{pid}\t2\tmissing\tunknown\n");
    let end = |calls, records, overflow| {
        format!("CX1\t{NONCE}\t{pid}\t{calls}\t{records}\t{overflow}\t1\n")
    };
    let invalid = [
        format!(
            "{ce1}{start_string}",
            start_string = String::from_utf8(start.clone()).unwrap()
        ),
        format!(
            "{}{}{}",
            String::from_utf8(start.clone()).unwrap(),
            ce1,
            ce1
        ),
        format!(
            "{}{}",
            String::from_utf8(start.clone()).unwrap(),
            end(2, 1, 0)
        ),
        format!(
            "{}{}{}",
            String::from_utf8(start.clone()).unwrap(),
            ce1,
            end(1, 0, 0)
        ),
        format!(
            "{}{}{}",
            String::from_utf8(start).unwrap(),
            ce1,
            end(1, 1, 1)
        ),
    ];
    for text in invalid {
        assert_eq!(
            client_reason_metadata::parse(text.as_bytes(), NONCE, &identity(pid)),
            Err("unavailable")
        );
    }
    let valid_two = format!("CR1\t{NONCE}\t{pid}\n{ce1}{ce2}{}", end(2, 2, 0));
    let observation =
        client_reason_metadata::parse(valid_two.as_bytes(), NONCE, &identity(pid)).unwrap();
    assert_eq!(observation.calls, 2);
    assert_eq!(observation.records.len(), 2);
    let overflow =
        format!("CR1\t{NONCE}\t{pid}\nCO1\t{NONCE}\t{pid}\nCX1\t{NONCE}\t{pid}\t4\t0\t1\t1\n");
    let observation =
        client_reason_metadata::parse(overflow.as_bytes(), NONCE, &identity(pid)).unwrap();
    assert!(observation.overflow && observation.complete && observation.healthy);
    assert_eq!(observation.calls, 4);
    let reversed = format!("CR1\t{NONCE}\t{pid}\n{ce2}{ce1}{}", end(2, 2, 0));
    let observation =
        client_reason_metadata::parse(reversed.as_bytes(), NONCE, &identity(pid)).unwrap();
    assert_eq!(
        observation
            .records
            .iter()
            .map(|record| record.call)
            .collect::<Vec<_>>(),
        [2, 1]
    );
    for invalid in [
        format!("CR1\t{NONCE}\t{pid}\r\n"),
        format!("CR1\t{NONCE}\t{pid}\nCE1\t{NONCE}\t{pid}\t+1\tverify\tissuer\n"),
        format!(
            "CR1\t{NONCE}\t{pid}\nCX1\t{NONCE}\t{pid}\t0\t0\t0\t1\nCE1\t{NONCE}\t{pid}\t1\tverify\tissuer\n"
        ),
        format!("CR1\t{NONCE}\t{pid}\nCO1\t{NONCE}\t{pid}\nCO1\t{NONCE}\t{pid}\n"),
    ] {
        assert_eq!(
            client_reason_metadata::parse(invalid.as_bytes(), NONCE, &identity(pid)),
            Err("unavailable")
        );
    }
}

#[test]
fn observer_sources_do_not_embed_cli_classifier_phrases() {
    const SOURCES: &[&[u8]] = &[
        include_bytes!("claude_two_tunnel/client_reason_classifier.cjs"),
        include_bytes!("claude_two_tunnel/client_reason_hook.cjs"),
        include_bytes!("claude_two_tunnel/client_reason_writer.cjs"),
        include_bytes!("claude_two_tunnel/client_reason_preload.cjs"),
    ];
    let forbidden = [
        ["CERTIFICATE", "_VERIFY_FAILED"].concat(),
        ["self-signed", " certificate"].concat(),
        "unable to verify".to_owned(),
    ];
    for source in SOURCES {
        for phrase in &forbidden {
            assert!(
                !source
                    .windows(phrase.len())
                    .any(|window| window.eq_ignore_ascii_case(phrase.as_bytes()))
            );
        }
    }
}
