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

pub fn capture_event_rows(fixture: &base::Fixture) -> Vec<Vec<u64>> {
    use std::ffi::OsString;
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/capture_completion/oracle.py");
    let mut oracle = base::Owned::spawn(
        PathBuf::from("/usr/bin/python3"),
        vec![
            OsString::from("-I"),
            OsString::from("-S"),
            script.into_os_string(),
            fixture.root.as_os_str().to_owned(),
            OsString::from(base::OP),
        ],
        fixture.scratch.clone(),
    );
    oracle.0.close_stdin();
    assert!(matches!(
        oracle.0.wait_until(deadline(3), || false),
        Ok(hf_platform::process::WaitOutcome::Exited(
            hf_domain::process::ProcessExit::Code(0)
        ))
    ));
    let until = deadline(2);
    while !oracle.0.capture().stdout.eof || !oracle.0.capture().stderr.eof {
        assert!(
            Instant::now() < until,
            "capture event oracle drain deadline"
        );
        oracle
            .0
            .pump(until, 8192)
            .expect("drain capture event oracle");
    }
    let output = String::from_utf8(oracle.0.capture().stdout.bytes.clone()).unwrap();
    let events = output
        .lines()
        .next()
        .and_then(|line| line.split('|').nth(2))
        .expect("capture event output");
    events
        .split(';')
        .map(|event| {
            event
                .split(',')
                .map(|value| value.parse().unwrap())
                .collect()
        })
        .collect()
}

pub fn assert_capture_event_sequence(events: &[Vec<u64>]) {
    assert!(!events.is_empty(), "capture event history is present");
    let mut prior = [0_u64; 8];
    for (index, row) in events.iter().enumerate() {
        assert_eq!(row.len(), 8, "capture event shape");
        assert_eq!(row[0], index as u64 + 1, "revisions are contiguous");
        assert!(
            row[1] >= prior[1] && row[2] >= prior[2],
            "offsets are monotone"
        );
        assert!(
            row[1] - prior[1] <= 16_384 && row[2] - prior[2] <= 16_384,
            "each persisted append respects the reader batch bounds"
        );
        assert!(
            row[3] >= prior[3] && row[4] >= prior[4],
            "EOF observations are monotone"
        );
        assert!(row[5] >= prior[5], "known loss is monotone");
        assert!(
            row[6] == 0 || (row[3] == 1 && row[4] == 1),
            "terminal exit follows both EOF observations"
        );
        prior.copy_from_slice(row);
    }
}

pub fn assert_terminal_event_extension(
    original: &RecoveryReport,
    terminal: &RecoveryReport,
    events: &[Vec<u64>],
) {
    assert_capture_event_sequence(events);
    assert_eq!(events.len() as u32, terminal.record.events_count);
    let before = original.record.capture.as_ref().unwrap();
    let revision = before.progress.revision as usize;
    assert_eq!(revision as u32, original.record.events_count);
    assert_eq!(
        &events[revision - 1],
        &[
            before.progress.revision,
            before.progress.stdout,
            before.progress.stderr,
            u64::from(before.progress.stdout_eof),
            u64::from(before.progress.stderr_eof),
            u64::from(before.progress.loss),
            0,
            0,
        ]
    );
    assert!(
        events.len() >= revision + 2,
        "EOF and terminal are separate events"
    );
    assert_eof_terminal_suffix(events, revision);
}

pub fn finish_readonly_restart(
    fixture: &base::Fixture,
    original: &RecoveryReport,
    parent: &mut base::Owned,
    peer: &mut std::os::unix::net::UnixStream,
    stage: hf_domain::fixture::Stage,
) {
    base::kill_direct(parent);
    base::echo_gate(peer, stage, base::OP, base::NONCE);
    base::wait_identity_gone(
        original
            .record
            .ownership
            .owner
            .as_ref()
            .unwrap()
            .process
            .pid(),
    );
    let terminal = inspect(fixture);
    assert_eq!(
        terminal.capture,
        CaptureAssessment::RecordedTerminalExtentObserved
    );
    let capture = terminal.record.capture.as_ref().unwrap();
    assert!(capture.progress.stdout_eof && capture.progress.stderr_eof);
    assert_eq!(capture.exit, hf_domain::process::ProcessExit::Code(0));
    assert_eq!(
        terminal.record.ownership.owner,
        original.record.ownership.owner
    );
    assert_eq!(
        terminal.record.ownership.child,
        original.record.ownership.child
    );
    assert_eq!(
        terminal.record.ownership.request.generation,
        original.record.ownership.request.generation
    );
    let events = capture_event_rows(fixture);
    assert_terminal_event_extension(original, &terminal, &events);
    assert_effect_for(fixture, original.record.ownership.child.as_ref().unwrap());
}

fn assert_eof_terminal_suffix(events: &[Vec<u64>], revision: usize) {
    let eof_index = events.len() - 2;
    for event in &events[revision..eof_index] {
        assert_eq!((event[3], event[4], event[5], event[6]), (0, 0, 0, 0));
    }
    let prior = &events[eof_index - 1];
    let eof = &events[eof_index];
    let terminal = &events[eof_index + 1];
    assert_eq!(
        (eof[1], eof[2]),
        (prior[1], prior[2]),
        "EOF event adds no bytes"
    );
    assert_eq!((eof[3], eof[4], eof[5], eof[6], eof[7]), (1, 1, 0, 0, 0));
    assert_eq!((terminal[1], terminal[2]), (eof[1], eof[2]));
    assert_eq!((terminal[3], terminal[4], terminal[5]), (1, 1, 0));
    assert_eq!((terminal[6], terminal[7]), (1, 0));
}
