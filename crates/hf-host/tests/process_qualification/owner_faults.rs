use super::support::*;
use hf_domain::fixture::{FixtureFault, Frame, ReportKind};
use std::{
    collections::BTreeSet,
    fs,
    os::unix::fs::MetadataExt,
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
fn owner_capacity_exhaustion_refuses_before_claim_child_or_effect() {
    let scratch = Scratch::new();
    let root = scratch.root();
    let effects = scratch.0.join("r03-b2.effects");
    let ledger = ledger(&effects);
    let mut parent = launch_parent(
        &scratch,
        &parent_plan(&root, ledger, None, FixtureFault::OwnerCapacityExhausted),
    );
    assert!(matches!(
        response(&parent.0),
        Frame::Report {
            kind: ReportKind::Uncertain,
            ..
        }
    ));
    finish_protocol(&mut parent);

    let stored = run_oracle(&root, OP, &scratch.0);
    assert!(
        stored.contains("\"phase\": 0"),
        "capacity refusal claimed: {stored}"
    );
    assert!(
        stored.contains("\"owner_process\": null"),
        "owner persisted: {stored}"
    );
    assert!(
        stored.contains("\"child\": null"),
        "child persisted: {stored}"
    );
    assert_eq!(fs::read_to_string(&effects).unwrap(), "");
    assert_full_fixture_fd_pool(&scratch.0, &root);
    assert!(!scratch.0.join(format!("r03-b2c.{OP}.stdout.lock")).exists());
    assert!(!scratch.0.join(format!("r03-b2c.{OP}.stderr.lock")).exists());
    assert!(!scratch.0.join(format!("r03-b2c.{OP}.stdout")).exists());
    assert!(!scratch.0.join(format!("r03-b2c.{OP}.stderr")).exists());
}

fn assert_full_fixture_fd_pool(scratch: &std::path::Path, root: &std::path::Path) {
    let mut identities = BTreeSet::new();
    for index in 0..254 {
        let path = scratch.join(format!("r04-d.capacity.{index:03}.lock"));
        insert_single_link_file(&path, &mut identities);
    }
    for name in ["store.lock", "io.lock"] {
        insert_single_link_file(&root.join(name), &mut identities);
    }
    assert_eq!(
        identities.len(),
        256,
        "pool paths must have distinct file identities"
    );
}

fn insert_single_link_file(path: &std::path::Path, identities: &mut BTreeSet<(u64, u64)>) {
    let metadata = fs::symlink_metadata(path).expect("exhaustion lock file exists");
    assert!(
        metadata.file_type().is_file(),
        "lock path is not a regular file: {}",
        path.display()
    );
    assert_eq!(
        metadata.nlink(),
        1,
        "lock file has aliases: {}",
        path.display()
    );
    assert!(
        identities.insert((metadata.dev(), metadata.ino())),
        "lock file identity is aliased: {}",
        path.display()
    );
}

#[test]
fn saturated_fixed_effect_keeps_claim_and_incomplete_capture_without_replay_effect() {
    let scratch = Scratch::new();
    let root = scratch.root();
    let effects = scratch.0.join("r03-b2.effects");
    let ledger = ledger(&effects);
    let mut parent = launch_parent(
        &scratch,
        &parent_plan(&root, ledger, None, FixtureFault::OutputSaturation),
    );
    assert!(matches!(
        response(&parent.0),
        Frame::Report {
            kind: ReportKind::Uncertain,
            ..
        }
    ));
    finish_protocol(&mut parent);

    let stored = run_oracle(&root, OP, &scratch.0);
    assert!(
        stored.contains("\"phase\": 3"),
        "effect acceptance was lost: {stored}"
    );
    assert!(
        stored.contains("\"child\": \""),
        "accepted child missing: {stored}"
    );
    assert_eq!(fs::read_to_string(&effects).unwrap().lines().count(), 1);
    super::success::replay_is_existing(
        &scratch,
        &root,
        ledger,
        oracle_generation(&stored),
        &effects,
        1,
    );
    let capture = capture_oracle(&root, &scratch.0);
    assert_incomplete_capture(&capture, &scratch.0);
}

fn assert_incomplete_capture(capture: &str, scratch: &std::path::Path) {
    let mut lines = capture.lines();
    let progress = lines.next().expect("capture progress line");
    let owner = lines.next().expect("retained owner row");
    assert!(
        owner.starts_with("owner:3:0:none:"),
        "reservation was released: {capture}"
    );
    let (kind, rest) = progress.split_once('|').expect("capture fields");
    let (row, events) = rest.split_once('|').expect("capture events");
    assert_eq!(
        kind, "progress",
        "overflow must not publish terminal capture: {capture}"
    );
    let values: Vec<u64> = row.split(',').map(|value| value.parse().unwrap()).collect();
    assert_incomplete_progress(&values, capture);
    assert_capture_events_and_raw(events, &values, capture, scratch);
}

fn assert_incomplete_progress(values: &[u64], capture: &str) {
    assert_eq!(values.len(), 8);
    assert!(values[0] > 0, "loss must be durably published: {capture}");
    assert!(
        values[1] > 0 && values[2] > 0,
        "some bounded output should persist: {capture}"
    );
    assert!(
        values[1] <= 65_536 && values[2] <= 65_536,
        "raw cap exceeded: {capture}"
    );
    assert_eq!(
        values[3], 0,
        "output EOF is not observed at the raw cap: {capture}"
    );
    assert_eq!(
        values[4], 0,
        "output EOF is not observed at the raw cap: {capture}"
    );
    assert_eq!(
        values[5], 0,
        "unknown loss must not be fabricated: {capture}"
    );
    assert_eq!(values[6], 0, "no terminal exit may be invented: {capture}");
}

fn assert_capture_events_and_raw(
    events: &str,
    values: &[u64],
    capture: &str,
    scratch: &std::path::Path,
) {
    let event_rows: Vec<Vec<u64>> = events
        .split(';')
        .map(|event| {
            event
                .split(',')
                .map(|value| value.parse().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(
        event_rows.len() as u64,
        values[0],
        "one event per revision: {capture}"
    );
    assert_eq!(
        event_rows.last().unwrap(),
        &values,
        "last event matches durable progress: {capture}"
    );
    assert_eq!(
        fs::metadata(scratch.join(format!("r03-b2c.{OP}.stdout")))
            .unwrap()
            .len(),
        values[1]
    );
    assert_eq!(
        fs::metadata(scratch.join(format!("r03-b2c.{OP}.stderr")))
            .unwrap()
            .len(),
        values[2]
    );
    assert!(
        values[1] < 73_728 && values[2] < 73_728,
        "helper output was not truncated by bounded capture"
    );
}

fn capture_oracle(root: &std::path::Path, cwd: &std::path::Path) -> String {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/capture_completion/oracle.py");
    let args = vec![
        "-I".into(),
        "-S".into(),
        script.into_os_string(),
        root.as_os_str().into(),
        OP.into(),
    ];
    let mut oracle = ChildGuard::spawn(PathBuf::from(PYTHON), args, cwd.to_owned());
    oracle.0.close_stdin();
    oracle.finish();
    let end = Instant::now() + Duration::from_secs(2);
    while !oracle.0.capture().stdout.eof || !oracle.0.capture().stderr.eof {
        assert!(Instant::now() < end, "bounded capture oracle drain expired");
        oracle.0.pump(end, 8192).expect("drain capture oracle");
    }
    assert!(!oracle.0.capture().stdout.overflow && !oracle.0.capture().stderr.overflow);
    String::from_utf8(oracle.0.capture().stdout.bytes.clone())
        .unwrap()
        .trim()
        .to_owned()
}
