#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Malformed, oversized and non-regular acceptance records are `Incompatible` at every
//! entry and at each later `read()`.
mod common;
mod read_session_support;
mod wal_acceptance_support;
use hf_storage::ReadRefusal;
use read_session_support::seeded;
use std::{fs, os::unix::fs::symlink};
use wal_acceptance_support::{
    RECORD, assert_refused_everywhere, generation, is_incompatible, put_bytes, record_bytes,
};

fn assert_incompatible(seeded: &read_session_support::Seeded, bytes: &[u8], label: &str) {
    assert_refused_everywhere(
        &seeded.root,
        |root| put_bytes(root, bytes),
        is_incompatible,
        &ReadRefusal::InvalidMarker,
        label,
    );
}

#[test]
fn a_foreign_generation_is_incompatible_even_when_accepted() {
    let seeded = seeded(3);
    let other = "f".repeat(32);
    assert_ne!(other, generation(&seeded.root));
    for state in ["accepted", "pending", "failed"] {
        assert_incompatible(&seeded, &record_bytes(&other, state), state);
    }
}

#[test]
fn malformed_content_is_incompatible() {
    let seeded = seeded(3);
    let generation = generation(&seeded.root);
    let valid = String::from_utf8(record_bytes(&generation, "accepted")).unwrap();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("wrong header", valid.replace("-1\n", "-2\n").into_bytes()),
        ("no trailing newline", valid.trim_end().as_bytes().to_vec()),
        ("extra line", format!("{valid}extra\n").into_bytes()),
        ("blank extra line", format!("{valid}\n").into_bytes()),
        ("unknown state", record_bytes(&generation, "done")),
        ("empty state", record_bytes(&generation, "")),
        ("upper case state", record_bytes(&generation, "ACCEPTED")),
        ("crlf", valid.replace('\n', "\r\n").into_bytes()),
        (
            "missing generation",
            b"harflow-wal-acceptance-1\naccepted\n".to_vec(),
        ),
        ("header only", b"harflow-wal-acceptance-1\n".to_vec()),
        ("non-utf8", {
            let mut bytes = record_bytes(&generation, "accepted");
            bytes.splice(0..0, [0xff, 0xfe]);
            bytes
        }),
        ("non-utf8 state", {
            let mut bytes = format!("harflow-wal-acceptance-1\n{generation}\n").into_bytes();
            bytes.extend([0xc3, 0x28, b'\n']);
            bytes
        }),
    ];
    for (label, bytes) in &cases {
        assert_incompatible(&seeded, bytes, label);
    }
}

#[test]
fn the_size_bound_is_128_bytes() {
    let seeded = seeded(3);
    let generation = generation(&seeded.root);
    let prefix = format!("harflow-wal-acceptance-1\n{generation}\n").into_bytes();
    // At and around the bound the content is still invalid, but never an I/O fault:
    // 128 bytes is read in full and rejected by the parser, 129 and more by the bound.
    for length in [127, 128, 129, 4096, 1 << 20] {
        let mut bytes = prefix.clone();
        bytes.resize(length, b'x');
        assert_incompatible(&seeded, &bytes, &format!("{length} bytes"));
    }
}

#[test]
fn a_symlink_is_incompatible_even_to_a_valid_record() {
    let seeded = seeded(3);
    let target = seeded.scratch.path().join("elsewhere-record");
    fs::write(&target, record_bytes(&generation(&seeded.root), "accepted")).unwrap();
    assert_refused_everywhere(
        &seeded.root,
        |root| symlink(&target, root.join(RECORD)).unwrap(),
        is_incompatible,
        &ReadRefusal::InvalidMarker,
        "symlink",
    );
    assert!(target.exists());
}

#[test]
fn a_dangling_symlink_is_incompatible() {
    let seeded = seeded(3);
    let target = seeded.scratch.path().join("no-such-record");
    assert_refused_everywhere(
        &seeded.root,
        |root| symlink(&target, root.join(RECORD)).unwrap(),
        is_incompatible,
        &ReadRefusal::InvalidMarker,
        "dangling symlink",
    );
}

#[test]
fn a_directory_is_incompatible() {
    let seeded = seeded(3);
    assert_refused_everywhere(
        &seeded.root,
        |root| fs::create_dir(root.join(RECORD)).unwrap(),
        is_incompatible,
        &ReadRefusal::InvalidMarker,
        "directory",
    );
}
