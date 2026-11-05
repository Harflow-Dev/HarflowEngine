#[allow(dead_code, unused_imports)]
#[path = "../handoff_losses/support.rs"]
mod base;
use base::{Owned, deadline, drain_capture};
use hf_domain::process::ProcessExit;
use hf_platform::fixture::{FIXED_STDERR, FIXED_STDOUT};
use hf_platform::process::WaitOutcome;
use std::path::PathBuf;

pub use base::{
    Fixture, NONCE, OP, SESSION, echo_gate, finish, gate_peer, kill_direct, process_child_pid,
    request_for, response, wait_identity_gone,
};

pub fn raw_bytes(fixture: &Fixture, stream: &str) -> Vec<u8> {
    let path = fixture.scratch.join(format!("r03-b2c.{}.{}", OP, stream));
    std::fs::read(path).unwrap()
}

pub fn assert_raw_prefix(stdout: &[u8], stderr: &[u8]) {
    assert!(
        FIXED_STDOUT.starts_with(stdout),
        "unexpected stdout bytes: {stdout:?}"
    );
    assert!(
        FIXED_STDERR.starts_with(stderr),
        "unexpected stderr bytes: {stderr:?}"
    );
    assert!(
        !stdout.is_empty() || !stderr.is_empty(),
        "append gate had no captured bytes"
    );
}

pub fn assert_raw_fixed(fixture: &Fixture) {
    assert_eq!(raw_bytes(fixture, "stdout"), FIXED_STDOUT);
    assert_eq!(raw_bytes(fixture, "stderr"), FIXED_STDERR);
}

pub fn assert_initial_capture(fixture: &Fixture) {
    let value = capture_oracle(fixture);
    let capture = value.lines().next().unwrap();
    let (kind, rest) = capture.split_once('|').expect("capture oracle kind");
    let (row, events) = rest.split_once('|').expect("capture oracle events");
    assert_eq!(kind, "progress");
    assert_eq!(row, "0,0,0,0,0,0,0,0");
    assert_eq!(events, "", "begin must not emit a capture event");
}

pub fn assert_incomplete(fixture: &Fixture, stdout: u64, stderr: u64) {
    let value = capture_oracle(fixture);
    let capture = value.lines().next().unwrap();
    let (kind, rest) = capture.split_once('|').expect("capture oracle kind");
    let (row, events) = rest.split_once('|').expect("capture oracle events");
    assert_eq!(
        kind, "progress",
        "terminal record appeared before finish: {value}"
    );
    let fields: Vec<u64> = row.split(',').map(|field| field.parse().unwrap()).collect();
    assert!(fields[0] > 0, "progress revision missing: {value}");
    assert_eq!(fields[1..], [stdout, stderr, 1, 1, 0, 0, 0], "{value}");
    let records: Vec<Vec<u64>> = events
        .split(';')
        .map(|event| {
            event
                .split(',')
                .map(|field| field.parse().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(
        records.len() as u64,
        fields[0],
        "every revision has one event: {value}"
    );
    for (index, event) in records.iter().enumerate() {
        assert_eq!(
            event[0],
            index as u64 + 1,
            "event revisions are contiguous: {value}"
        );
    }
}

pub fn assert_published_matches_raw(fixture: &Fixture, stdout: &[u8], stderr: &[u8]) {
    let value = capture_oracle(fixture);
    let capture = value.lines().next().unwrap();
    let (kind, rest) = capture.split_once('|').expect("capture oracle kind");
    let (row, events) = rest.split_once('|').expect("capture oracle events");
    assert_eq!(
        kind, "progress",
        "terminal record appeared before finish: {value}"
    );
    let fields: Vec<u64> = row.split(',').map(|field| field.parse().unwrap()).collect();
    assert_eq!(
        fields[1],
        stdout.len() as u64,
        "stored stdout offset differs from raw bytes: {value}"
    );
    assert_eq!(
        fields[2],
        stderr.len() as u64,
        "stored stderr offset differs from raw bytes: {value}"
    );
    assert_eq!(
        fields[5..],
        [0, 0, 0],
        "loss or exit was fabricated: {value}"
    );
    assert_eq!(
        events.split(';').count() as u64,
        fields[0],
        "one event per revision: {value}"
    );
}

pub fn assert_terminal(fixture: &Fixture, stdout: u64, stderr: u64) {
    let value = capture_oracle(fixture);
    let capture = value.lines().next().unwrap();
    let (kind, rest) = capture.split_once('|').expect("capture oracle kind");
    let (row, events) = rest.split_once('|').expect("capture oracle events");
    assert_eq!(kind, "terminal", "terminal completion missing: {value}");
    let fields: Vec<u64> = row.split(',').map(|field| field.parse().unwrap()).collect();
    assert_eq!(fields[1..], [stdout, stderr, 1, 1, 0, 1, 0], "{value}");
    let records: Vec<Vec<u64>> = events
        .split(';')
        .map(|event| {
            event
                .split(',')
                .map(|field| field.parse().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(
        records.len() as u64,
        fields[0],
        "every revision has one event: {value}"
    );
    for (index, event) in records.iter().enumerate() {
        assert_eq!(
            event[0],
            index as u64 + 1,
            "event revisions are contiguous: {value}"
        );
    }
    assert_eq!(
        records.last().unwrap(),
        &fields,
        "terminal event and row must commit together: {value}"
    );
}

pub fn assert_owner_accepted(fixture: &Fixture) {
    let value = capture_oracle(fixture);
    let owner = value.lines().nth(1).expect("owner oracle record");
    let fields: Vec<_> = owner.split(':').collect();
    assert_eq!(fields.len(), 5);
    assert_eq!(fields[0], "owner");
    assert_eq!(
        fields[1], "3",
        "acceptance reservation remains held: {value}"
    );
    assert_eq!(fields[2], "0", "owner was not cancelled: {value}");
    assert_eq!(
        fields[3], "none",
        "no release proof was fabricated: {value}"
    );
}

pub fn stored_generation(fixture: &Fixture) -> hf_domain::ownership::Id {
    let value = capture_oracle(fixture);
    let generation = value.lines().nth(1).unwrap().split(':').nth(4).unwrap();
    hf_domain::ownership::Id::new(generation.to_owned()).unwrap()
}

pub fn capture_oracle(fixture: &Fixture) -> String {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/capture_completion/oracle.py");
    let mut child = Owned::spawn(
        PathBuf::from("/usr/bin/python3"),
        vec![
            "-I".into(),
            "-S".into(),
            script.into_os_string(),
            fixture.root.as_os_str().into(),
            OP.into(),
        ],
        fixture.scratch.clone(),
    );
    child.0.close_stdin();
    let until = deadline(3);
    assert!(matches!(
        child.0.wait_until(until, || false),
        Ok(WaitOutcome::Exited(ProcessExit::Code(0)))
    ));
    drain_capture(&mut child, until).unwrap();
    assert!(child.0.capture().stdout.eof && child.0.capture().stderr.eof);
    assert!(!child.0.capture().stdout.overflow && !child.0.capture().stderr.overflow);
    String::from_utf8(child.0.capture().stdout.bytes.clone())
        .unwrap()
        .trim()
        .to_owned()
}
