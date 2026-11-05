#![allow(dead_code, unused_imports)]

#[path = "../handoff_losses/support.rs"]
pub mod base;

use hf_application::recovery::RecoveryInspector;
use hf_domain::process::ProcessExit;
use hf_domain::{ownership::Id, recovery::RecoveryReport};
use hf_platform::process::WaitOutcome;
use hf_platform::recovery::{MacRecoveryIdentity, RawCaptureObserver};
use hf_storage::{Limits, RecoveryReadSession};
use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

pub use base::{NONCE, OP, SESSION};

pub fn inspect(fixture: &base::Fixture) -> RecoveryReport {
    let store = RecoveryReadSession::new(&fixture.root, &fixture.scratch, Limits::default());
    let inspector = RecoveryInspector::new(
        store,
        MacRecoveryIdentity,
        RawCaptureObserver::new(&fixture.root),
    );
    inspector
        .inspect(&id(OP), deadline())
        .expect("bounded read-only observation")
        .expect("prepared operation remains present")
}

pub fn file_state(fixture: &base::Fixture) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    let mut paths = vec![
        fixture.root.join("store.sqlite3"),
        fixture.root.join("store.sqlite3-wal"),
        fixture.root.join("store.sqlite3-shm"),
        fixture.root.join("store.identity"),
        fixture.root.join("store.lock"),
        fixture.root.join("io.lock"),
        fixture.scratch.join(format!("r03-b2c.{OP}.stdout")),
        fixture.scratch.join(format!("r03-b2c.{OP}.stderr")),
    ];
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let bytes = match fs::read(&path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => panic!("cannot independently read {}: {error}", path.display()),
            };
            (path, bytes)
        })
        .collect()
}

pub fn raw_bytes(fixture: &base::Fixture, stream: &str) -> Vec<u8> {
    fs::read(fixture.scratch.join(format!("r03-b2c.{OP}.{stream}"))).expect("raw fixture bytes")
}

pub fn capture_oracle(fixture: &base::Fixture) -> String {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/capture_completion/oracle.py");
    let mut child = base::Owned::spawn(
        PathBuf::from("/usr/bin/python3"),
        vec![
            OsString::from("-I"),
            OsString::from("-S"),
            script.into_os_string(),
            fixture.root.as_os_str().to_owned(),
            OsString::from(OP),
        ],
        fixture.scratch.clone(),
    );
    child.0.close_stdin();
    let until = Instant::now() + Duration::from_secs(2);
    assert!(
        matches!(
            child.0.wait_until(until, || false),
            Ok(WaitOutcome::Exited(ProcessExit::Code(0)))
        ),
        "independent read-only SQLite oracle failed"
    );
    base::drain_capture(&mut child, until).expect("bounded oracle output drain");
    assert!(child.0.capture().stdout.eof && child.0.capture().stderr.eof);
    assert!(!child.0.capture().stdout.overflow && !child.0.capture().stderr.overflow);
    String::from_utf8(child.0.capture().stdout.bytes.clone())
        .expect("oracle output is UTF-8")
        .trim()
        .to_owned()
}

fn id(value: &str) -> Id {
    Id::new(value.to_owned()).expect("fixed operation ID")
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
