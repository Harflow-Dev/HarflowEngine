//! The real writer service on a thread, driven by the test as owner.
use super::{
    driver::{Driver, init_line, result_line, text_line},
    flush::{Arm, ArmedFlush},
    harness::{Ctx, Probe, Seed, WriterConfig},
    serial_case,
};
use hf_domain::{process::ProcessExit, task_runtime::CaptureLoss};
use hf_storage::WalMaintenance;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

const VERSION: &str = hf_backends::backend_launch::CLAUDE_VERSION;

fn seeded(name: &str) -> (Ctx, Seed) {
    let ctx = Ctx::new(name);
    let seed = ctx.seed("read", VERSION);
    (ctx, seed)
}
fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(15);
    while !ready() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}
/// A config whose idle maintenance reports `Idle` and counts its calls.
fn idle_counting() -> (WriterConfig, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let config = WriterConfig {
        idle: Some(Arc::new(move || {
            seen.fetch_add(1, Ordering::AcqRel);
            Some(Ok(WalMaintenance::Idle))
        })),
        ..WriterConfig::default()
    };
    (config, calls)
}
fn status(probe: &Probe) -> Option<String> {
    probe.task_status()
}

#[test]
fn malformed_agent_output_marks_failed_and_keeps_draining() {
    let _serial = serial_case();
    let (ctx, seed) = seeded("malformed");
    let (config, idle) = idle_counting();
    let mut driver = Driver::start(&ctx, &seed, &config);
    driver.spawn_child();
    assert!(!driver.capture(&init_line()).close_input);
    let before = idle.load(Ordering::Acquire);
    let ack = driver.capture(b"this is not json\n");
    assert!(ack.close_input, "malformed output closes the agent's input");
    let first = ack.stdout_end;
    let ack = driver.capture(b"more bytes after the fault\n");
    assert!(
        ack.close_input && ack.stdout_end > first,
        "raw bytes keep growing"
    );
    // The writer is not poisoned: its idle maintenance keeps running.
    wait_until("idle maintenance after the fault", || {
        idle.load(Ordering::Acquire) > before
    });
    assert!(
        driver
            .finish(ProcessExit::Code(0), CaptureLoss::None)
            .terminal
    );
    assert_eq!(
        driver.wait_finished(),
        ProcessExit::Code(0),
        "final maintenance ran"
    );
    let evidence = seed.evidence();
    assert!(evidence.observation.observation_gap && evidence.observation.result.is_none());
    assert_eq!(evidence.observation.exit, ProcessExit::Code(0));
    assert_eq!(status(&seed).as_deref(), Some("failed"));
    assert!(!seed.status_text().unwrap().contains("\"summary\":\""));
    seed.assert_never_accepted();
}

#[test]
fn malformed_result_record_publishes_no_result() {
    let _serial = serial_case();
    let (ctx, seed) = seeded("malformed-result");
    // After the init line, the stream's next sequence number is pushed past the storage
    // range: the result line is valid and staged, then its event projection refuses. Once the
    // line is rejected the number is restored so that the terminal can be published.
    let saved = Arc::new(AtomicUsize::new(0));
    let config = WriterConfig {
        stream: Some(Arc::new(move |stream| {
            let past = i64::MAX as u64 + 1;
            if stream.malformed() && stream.next_seq >= past {
                stream.next_seq = saved.load(Ordering::Acquire) as u64;
            } else if stream.initialized && !stream.malformed() && stream.next_seq < past {
                saved.store(stream.next_seq as usize, Ordering::Release);
                stream.next_seq = past;
            }
        })),
        ..WriterConfig::default()
    };
    let mut driver = Driver::start(&ctx, &seed, &config);
    driver.spawn_child();
    driver.capture(&init_line());
    let ack = driver.capture(&result_line("\"done\""));
    assert!(ack.close_input);
    let evidence = seed.evidence();
    assert!(
        evidence.observation.result.is_none(),
        "no result was published"
    );
    assert!(evidence.observation.observation_gap);
    driver.finish(ProcessExit::Code(0), CaptureLoss::None);
    assert_eq!(status(&seed).as_deref(), Some("failed"));
    assert!(!seed.status_text().unwrap().contains("\"summary\":\""));
    seed.assert_never_accepted();
}

#[test]
fn malformed_after_result_is_batch_independent() {
    let _serial = serial_case();
    let mut valid = init_line();
    valid.extend(result_line("\"done\""));
    let garbage = b"garbage after the result\n".to_vec();
    let partial = br#"{"type":"resu"#.to_vec();
    let cases: Vec<(&str, Vec<Vec<u8>>)> = vec![
        ("same", vec![[valid.clone(), garbage.clone()].concat()]),
        ("split", vec![valid.clone(), garbage]),
        ("partial", vec![valid.clone(), partial]),
    ];
    for (name, batches) in cases {
        let (ctx, seed) = seeded(&format!("after-{name}"));
        let mut driver = Driver::start(&ctx, &seed, &WriterConfig::default());
        driver.spawn_child();
        for batch in &batches {
            driver.capture(batch);
        }
        driver.finish(ProcessExit::Code(0), CaptureLoss::None);
        let evidence = seed.evidence();
        assert!(
            evidence.observation.result.is_some(),
            "{name}: the result stands"
        );
        assert!(evidence.observation.observation_gap, "{name}: gap");
        // Q1(a): the task completes per the storage rule, without a summary.
        assert_eq!(status(&seed).as_deref(), Some("completed"), "{name}");
        assert!(
            !seed.status_text().unwrap().contains("\"summary\":\""),
            "{name}"
        );
        seed.assert_never_accepted();
    }
}

#[test]
fn append_uncertainty_never_becomes_terminal_fact() {
    let _serial = serial_case();
    for nth in [1, 2] {
        let (ctx, seed) = seeded(&format!("flush{nth}"));
        let arm = Arc::new(Arm::default());
        let snapshot = Arc::new(Mutex::new(None));
        let (observed, hook_arm) = (snapshot.clone(), arm.clone());
        let config = WriterConfig {
            options: Some(Arc::new(move |options| {
                let flush = Arc::new(ArmedFlush::new(options.flush.clone(), hook_arm.clone()));
                options.with_flush(flush)
            })),
            idle: Some(Arc::new(|| Some(Ok(WalMaintenance::Idle)))),
            retained: Some(Arc::new(move |runtime| {
                let poisoned = runtime.entry.writer().is_poisoned();
                *observed.lock().unwrap() = Some(poisoned);
            })),
            stream: None,
            exchange_delay: None,
        };
        let mut driver = Driver::start(&ctx, &seed, &config);
        driver.spawn_child();
        driver.capture(&init_line());
        arm.arm(nth);
        driver.send_capture(&text_line(1));
        wait_until("the writer to retain", || {
            snapshot.lock().unwrap().is_some()
        });
        assert_eq!(
            *snapshot.lock().unwrap(),
            Some(true),
            "flush {nth}: poisoned"
        );
        let pending = driver.receive_ack_within(Duration::from_millis(200));
        assert!(pending.is_err(), "flush {nth}: no acknowledgement follows");
        assert_eq!(status(&seed).as_deref(), Some("running"), "flush {nth}");
        seed.assert_never_accepted();
    }
}

#[test]
fn saturation_after_durable_result_completes_with_known_loss() {
    let _serial = serial_case();
    let (ctx, seed) = seeded("loss-after-result");
    let mut driver = Driver::start(&ctx, &seed, &WriterConfig::default());
    driver.spawn_child();
    driver.capture(&[init_line(), result_line("\"done\"")].concat());
    assert!(
        driver
            .finish(ProcessExit::Code(0), CaptureLoss::Stall)
            .terminal
    );
    let evidence = seed.evidence();
    assert!(evidence.observation.capture_known_loss && evidence.observation.observation_gap);
    assert!(evidence.observation.result.is_some());
    assert_eq!(status(&seed).as_deref(), Some("completed"));
    assert_eq!(
        std::fs::read(driver.raw_file("loss.raw")).unwrap(),
        b"capture-known-loss stall\n"
    );
    seed.assert_never_accepted();
}

#[test]
fn finish_command_to_terminal_ack_for_stall_and_disconnect() {
    let _serial = serial_case();
    for (name, cause, line) in [
        (
            "loss-stall",
            CaptureLoss::Stall,
            &b"capture-known-loss stall\n"[..],
        ),
        (
            "loss-disconnect",
            CaptureLoss::Disconnect,
            &b"capture-known-loss disconnect\n"[..],
        ),
    ] {
        let (ctx, seed) = seeded(name);
        let mut driver = Driver::start(&ctx, &seed, &WriterConfig::default());
        driver.spawn_child();
        driver.capture(&[init_line(), result_line("\"done\"")].concat());
        // Encoded by the driver, decoded by the writer's top-level decoder, then settled.
        assert!(driver.finish(ProcessExit::Code(0), cause).terminal);
        assert_eq!(std::fs::read(driver.raw_file("loss.raw")).unwrap(), line);
        let evidence = seed.evidence();
        assert!(evidence.observation.capture_known_loss && evidence.observation.observation_gap);
        assert_eq!(status(&seed).as_deref(), Some("completed"));
        seed.assert_never_accepted();
    }
}

#[test]
fn known_loss_recording_failure_retains_without_terminal() {
    let _serial = serial_case();
    let (ctx, seed) = seeded("loss-failure");
    let reached = Arc::new(AtomicBool::new(false));
    let flag = reached.clone();
    let config = WriterConfig {
        retained: Some(Arc::new(move |_| flag.store(true, Ordering::Release))),
        ..WriterConfig::default()
    };
    let mut driver = Driver::start(&ctx, &seed, &config);
    driver.spawn_child();
    driver.capture(&init_line());
    // The renamed `loss.raw` fails the known-loss append in `RawCapture::validate`.
    let loss = driver.raw_file("loss.raw");
    std::fs::rename(&loss, loss.with_extension("moved")).unwrap();
    driver.send_finish(ProcessExit::Code(1), CaptureLoss::Stall);
    wait_until("the writer to retain", || reached.load(Ordering::Acquire));
    assert!(
        driver
            .receive_ack_within(Duration::from_millis(200))
            .is_err()
    );
    assert_eq!(status(&seed).as_deref(), Some("running"), "no terminal");
    seed.assert_never_accepted();
}

#[test]
fn zero_exit_without_matched_result_is_failed() {
    let _serial = serial_case();
    let (ctx, seed) = seeded("no-result");
    let mut driver = Driver::start(&ctx, &seed, &WriterConfig::default());
    driver.spawn_child();
    driver.capture(&init_line());
    assert!(
        driver
            .finish(ProcessExit::Code(0), CaptureLoss::None)
            .terminal
    );
    assert_eq!(status(&seed).as_deref(), Some("failed"));
    let evidence = seed.evidence();
    assert_eq!(evidence.observation.exit, ProcessExit::Code(0));
    assert!(evidence.observation.result.is_none());
    seed.assert_never_accepted();
}

#[test]
fn idle_poisoning_error_retains_writer_without_ack() {
    let _serial = serial_case();
    let (ctx, seed) = seeded("idle-poison");
    let reached = Arc::new(AtomicBool::new(false));
    let flag = reached.clone();
    let config = WriterConfig {
        idle: Some(Arc::new(|| {
            Some(Err(hf_storage::Error::CommitOutcomeUnknown))
        })),
        retained: Some(Arc::new(move |_| flag.store(true, Ordering::Release))),
        ..WriterConfig::default()
    };
    let mut driver = Driver::start(&ctx, &seed, &config);
    wait_until("the writer to retain", || reached.load(Ordering::Acquire));
    driver.send_capture(&init_line());
    assert!(
        driver
            .receive_ack_within(Duration::from_millis(200))
            .is_err()
    );
    seed.assert_never_accepted();
}
