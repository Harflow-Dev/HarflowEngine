#[path = "../handoff_losses/support.rs"]
#[allow(dead_code, unused_imports)]
pub mod base;

use hf_application::process::IdentityPort;
use hf_application::recovery::RecoveryInspector;
use hf_domain::process::ProcessIdentity;
use hf_domain::{
    ownership::Id,
    recovery::{CaptureAssessment, ProcessState, RecoveryReport},
};
use hf_platform::process::MacIdentity;
use hf_platform::recovery::{MacRecoveryIdentity, RawCaptureObserver};
use hf_storage::{Limits, RecoveryReadSession};
use std::{
    fs, io,
    path::PathBuf,
    time::{Duration, Instant},
};

pub fn inspect(fixture: &base::Fixture) -> RecoveryReport {
    let store = RecoveryReadSession::new(
        &fixture.root,
        &fixture.scratch,
        Limits::default(),
        hf_platform::lease::shared(),
    );
    RecoveryInspector::new(
        store,
        MacRecoveryIdentity,
        RawCaptureObserver::new(&fixture.root),
    )
    .inspect(&Id::new(base::OP.to_owned()).unwrap(), deadline(2))
    .expect("bounded read-only inspection")
    .expect("operation is retained")
}

pub fn stable_projection(report: &RecoveryReport) -> String {
    format!(
        "{:?}",
        (
            &report.record.source,
            &report.record.ownership,
            &report.record.capture,
            report.record.events_count,
            &report.record.events_digest,
            &report.raw,
            report.capture,
            report.reservation,
        )
    )
}

pub fn assert_identity_consistency(report: &RecoveryReport, owner_must_be_live: bool) {
    let owner = &report
        .record
        .ownership
        .owner
        .as_ref()
        .expect("stored owner")
        .process;
    let evidence = report.owner.as_ref().expect("observed owner");
    assert_eq!(&evidence.expected, owner);
    if owner_must_be_live {
        assert_eq!(evidence.state, ProcessState::Present);
        assert_eq!(evidence.actual.as_ref(), Some(owner));
    }
    if let Some(actual) = &evidence.actual {
        assert_eq!(actual, owner);
    }
    if let Some(child) = &report.record.ownership.child {
        let evidence = report.child.as_ref().expect("observed stored child");
        assert_eq!(&evidence.expected, child);
        if let Some(actual) = &evidence.actual {
            assert_eq!(actual, child);
        }
    }
}

pub fn file_state(fixture: &base::Fixture) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    let mut paths = vec![
        fixture.root.join("store.sqlite3"),
        fixture.root.join("store.sqlite3-wal"),
        fixture.root.join("store.sqlite3-shm"),
        fixture.root.join("store.identity"),
        fixture.root.join("store.lock"),
        fixture.root.join("io.lock"),
        fixture.scratch.join(format!("r03-b2c.{}.stdout", base::OP)),
        fixture.scratch.join(format!("r03-b2c.{}.stderr", base::OP)),
    ];
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let bytes = match fs::read(&path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => panic!("cannot independently read {}: {error}", path.display()),
            };
            (path, bytes)
        })
        .collect()
}

pub fn store_namespace(fixture: &base::Fixture) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(&fixture.root)
        .expect("read isolated store namespace")
        .map(|entry| entry.expect("store directory entry").path())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

pub fn assert_effect_for(fixture: &base::Fixture, child: &ProcessIdentity) {
    let expected = format!(
        "{}|{}|{}|{}|{}\n",
        base::OP,
        child.pid(),
        child.start_seconds(),
        child.start_microseconds(),
        child.boot_uuid()
    );
    assert_eq!(fs::read_to_string(&fixture.effects).unwrap(), expected);
}

pub fn waiting_owner_child(
    fixture: &base::Fixture,
    report: &RecoveryReport,
) -> (ProcessIdentity, ProcessIdentity) {
    let owner = report
        .record
        .ownership
        .owner
        .as_ref()
        .unwrap()
        .process
        .clone();
    assert_eq!(report.owner.as_ref().unwrap().state, ProcessState::Present);
    assert_eq!(MacIdentity.identity(owner.pid()), Some(owner.clone()));
    let child_pid = base::process_child_pid(
        owner.pid(),
        "--internal-effect-fixture-v1",
        &fixture.scratch,
    );
    let child = MacIdentity
        .identity(child_pid)
        .expect("qualified original waiting child identity");
    assert_eq!(
        report.record.ownership.phase,
        hf_domain::ownership::Phase::Claimed
    );
    assert!(
        report.record.ownership.child.is_none(),
        "spawn gate precedes the durable child record"
    );
    assert!(fs::read_to_string(&fixture.effects).unwrap().is_empty());
    (owner, child)
}

pub fn assert_terminal_replay(
    fixture: &base::Fixture,
    terminal: &RecoveryReport,
    owner: &ProcessIdentity,
    child: &ProcessIdentity,
) {
    assert_eq!(
        terminal.capture,
        CaptureAssessment::RecordedTerminalExtentObserved
    );
    assert_eq!(
        terminal.record.ownership.owner.as_ref().unwrap().process,
        *owner
    );
    assert_eq!(terminal.record.ownership.child.as_ref(), Some(child));
    assert_effect_for(fixture, child);
    let mut plan = fixture.plan_for(base::request_for(
        base::OP,
        base::SESSION,
        &terminal.record.ownership.request.generation,
    ));
    plan.initialize = false;
    let mut replay = fixture.parent(plan);
    assert!(matches!(
        base::response(&replay.0),
        hf_domain::fixture::Frame::Report {
            kind: hf_domain::fixture::ReportKind::Existing,
            ..
        }
    ));
    base::finish_protocol(&mut replay);
    assert_effect_for(fixture, child);
}

pub fn assert_live_capture(report: &RecoveryReport) {
    let owner = report
        .record
        .ownership
        .owner
        .as_ref()
        .expect("durable owner identity");
    assert_eq!(report.owner.as_ref().unwrap().state, ProcessState::Present);
    assert_eq!(
        report.owner.as_ref().unwrap().actual.as_ref(),
        Some(&owner.process)
    );
    assert_eq!(
        report.record.ownership.phase,
        hf_domain::ownership::Phase::AcceptanceRecorded
    );
    let child = report
        .record
        .ownership
        .child
        .as_ref()
        .expect("durable child identity");
    assert_eq!(report.child.as_ref().unwrap().expected, *child);
    assert_eq!(report.capture, CaptureAssessment::Incomplete);
    let capture = report
        .record
        .capture
        .as_ref()
        .expect("published capture snapshot");
    assert_eq!(
        capture.binding.fence,
        report.record.ownership.fence().unwrap()
    );
    assert_eq!(capture.exit, hf_domain::process::ProcessExit::Unobserved);
    assert_eq!(
        report.record.events_count as usize,
        capture.progress.revision as usize
    );
}

pub fn deadline(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds)
}
