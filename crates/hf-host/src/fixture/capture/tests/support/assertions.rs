use super::*;
use hf_domain::ownership::PreparedRequest;

pub(in crate::fixture::capture::tests) fn pattern(
    length: usize,
    multiplier: usize,
    addend: usize,
) -> Vec<u8> {
    (0..length)
        .map(|index| ((index * multiplier + addend) % 256) as u8)
        .collect()
}

pub(in crate::fixture::capture::tests) fn assert_raw_prefix(
    root: &Path,
    operation: &Id,
    counts: (u64, u64),
) {
    let stdout = fs::read(root.join(format!("r03-b2c.{}.stdout", operation.as_str()))).unwrap();
    let stderr = fs::read(root.join(format!("r03-b2c.{}.stderr", operation.as_str()))).unwrap();
    assert_eq!((stdout.len() as u64, stderr.len() as u64), counts);
    assert_eq!(stdout, pattern(SIZE, 17, 3)[..stdout.len()]);
    assert_eq!(stderr, pattern(SIZE, 29, 7)[..stderr.len()]);
}

pub(in crate::fixture::capture::tests) fn assert_known_loss(
    result: WorkerResult,
    calls: &CallCounts,
    scratch: &Scratch,
    request: &PreparedRequest,
) {
    let report = result.report;
    assert!(!report.terminal);
    assert_eq!(report.accounting.uncertain, (0, 0));
    assert_eq!(report.accounting.abandoned, (0, 0));
    assert!(report.accounting.acknowledged.0 > 0 && report.accounting.acknowledged.1 > 0);
    assert!(report.accounting.acknowledged.0 < SIZE as u64);
    assert!(report.accounting.acknowledged.1 < SIZE as u64);
    let stored = result.session.capture().unwrap();
    assert!(stored.progress.loss);
    assert!(!stored.progress.stdout_eof && !stored.progress.stderr_eof);
    assert_eq!(stored.exit, ProcessExit::Unobserved);
    assert_raw_prefix(
        &scratch.0,
        &request.operation,
        report.accounting.acknowledged,
    );
    assert_eq!(result.exit, ProcessExit::Code(0));
    let final_progress = &result.final_observation.as_ref().unwrap().progress;
    assert!(final_progress.stdout.eof && final_progress.stderr.eof);
    assert_eq!(calls.begins.load(Ordering::Relaxed), 1);
    assert_eq!(calls.finishes.load(Ordering::Relaxed), 0);
    drop(result.session);
}

pub(in crate::fixture::capture::tests) fn assert_poisoned_publish(
    result: WorkerResult,
    observed: &hf_platform::process::ReaderProgress,
    calls: &CallCounts,
    scratch: &Scratch,
    request: &PreparedRequest,
) {
    let report = result.report;
    assert!(!report.terminal);
    assert_eq!(result.exit, ProcessExit::Code(0));
    let final_progress = &result.final_observation.as_ref().unwrap().progress;
    assert!(final_progress.stdout.eof && final_progress.stderr.eof);
    assert!(result.session.is_poisoned());
    assert!(!result.session.capture().unwrap().progress.loss);
    assert_eq!(calls.begins.load(Ordering::Relaxed), 1);
    assert_eq!(calls.publishes.load(Ordering::Relaxed), 1);
    assert_eq!(calls.finishes.load(Ordering::Relaxed), 0);
    assert_eq!(report.accounting.acknowledged, (0, 0));
    assert!(report.accounting.uncertain.0 > 0 && report.accounting.uncertain.1 > 0);
    assert!(report.accounting.abandoned.0 > 0 && report.accounting.abandoned.1 > 0);
    assert_eq!(
        report.accounting.uncertain.0 + report.accounting.abandoned.0,
        observed.stdout.enqueued
    );
    assert_eq!(
        report.accounting.uncertain.1 + report.accounting.abandoned.1,
        observed.stderr.enqueued
    );
    assert_raw_prefix(&scratch.0, &request.operation, report.accounting.uncertain);

    let operation = request.operation.clone();
    drop(result.session);
    assert_eq!(calls.publishes.load(Ordering::Relaxed), 1);
    assert_eq!(calls.finishes.load(Ordering::Relaxed), 0);
    let reopened = Store::open(
        scratch.root(),
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
        hf_platform::lease::shared(),
    )
    .unwrap();
    let persisted = reopened
        .capture_observe(&operation, Instant::now() + Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(
        (persisted.progress.stdout, persisted.progress.stderr),
        report.accounting.uncertain
    );
    assert!(!persisted.progress.loss);
    assert!(!persisted.progress.stdout_eof && !persisted.progress.stderr_eof);
    assert_eq!(persisted.exit, ProcessExit::Unobserved);
}
