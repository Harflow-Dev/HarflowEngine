#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::common::*;
use hf_domain::process::{IdentityObservation, ProcessIdentity};
use hf_platform::process::{
    MacIdentity, OwnedSignal, RetainedOwnedChild, SignalOutcome, SpawnInitialization,
};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn registered_identity_matches_independent_bsdinfo_and_boot_uuid() {
    let mut child = spawn("hold", &[], 4096);
    pump_until(&mut child, |value| {
        value.capture().stdout.bytes.starts_with(b"READY\n")
    });
    let identity = child.registered_identity().unwrap().clone();
    let (seconds, micros, status) = independent_os_probe(child.pid());
    assert_eq!(
        (
            identity.pid(),
            identity.start_seconds(),
            identity.start_microseconds()
        ),
        (child.pid(), seconds, micros)
    );
    assert_eq!(identity.boot_uuid(), independent_boot_uuid());
    assert_ne!(
        status, 5,
        "fixture unexpectedly observed as zombie before product wait"
    );
    cooperative_quit(&mut child);
}

#[test]
fn initial_unknown_identity_retains_child_and_refuses_control() {
    let child = RetainedOwnedChild::spawn(
        spec("hold", &[], 4096),
        &FixedIdentity(None),
        deadline(3_000),
    )
    .unwrap();
    let mut child = FixtureGuard(child);
    assert_eq!(child.initialization(), SpawnInitialization::UnknownIdentity);
    assert!(child.registered_identity().is_none());

    let (seconds, micros, status) = independent_os_probe(child.pid());
    let expected =
        ProcessIdentity::new(child.pid(), seconds, micros, independent_boot_uuid()).unwrap();
    assert_ne!(
        status, 5,
        "unknown-identity fixture unexpectedly observed as zombie"
    );
    pump_until(&mut child, |value| {
        value.capture().stdout.bytes.starts_with(b"READY\n")
    });

    let refusal = child
        .request_signal(
            &expected,
            &MacIdentity,
            OwnedSignal::Terminate,
            deadline(250),
        )
        .unwrap();
    assert_eq!(
        refusal,
        SignalOutcome::Refused(IdentityObservation::Mismatch)
    );
    let nonce = format!("{:032x}", u128::from(child.pid()) + 7);
    let expected_echo = format!("PONG {nonce}\n").into_bytes();
    write_all(&mut child, format!("ping {nonce}\n").as_bytes());
    pump_until(&mut child, |value| {
        value
            .capture()
            .stdout
            .bytes
            .windows(expected_echo.len())
            .any(|part| part == expected_echo.as_slice())
    });
    cooperative_quit(&mut child);
}

#[test]
fn stale_reboot_unknown_and_expired_signals_refuse_while_sentinel_survives() {
    let mut target = spawn("hold", &[], 4096);
    let mut sentinel = spawn("hold", &[], 4096);
    pump_until(&mut target, |value| {
        value.capture().stdout.bytes.starts_with(b"READY\n")
    });
    pump_until(&mut sentinel, |value| {
        value.capture().stdout.bytes.starts_with(b"READY\n")
    });
    let expected = target.registered_identity().unwrap().clone();
    let other_pid = sentinel.registered_identity().unwrap().clone();
    assert_eq!(
        target
            .request_signal(
                &other_pid,
                &MacIdentity,
                OwnedSignal::Terminate,
                deadline(100)
            )
            .unwrap(),
        SignalOutcome::Refused(IdentityObservation::Mismatch)
    );
    let stale = ProcessIdentity::new(
        expected.pid(),
        expected.start_seconds() + 1,
        expected.start_microseconds(),
        expected.boot_uuid().to_owned(),
    )
    .unwrap();
    let rebooted = ProcessIdentity::new(
        expected.pid(),
        expected.start_seconds(),
        expected.start_microseconds(),
        "11111111-2222-3333-4444-555555555555".into(),
    )
    .unwrap();
    for fake in [Some(stale), Some(rebooted), None] {
        let result = target
            .request_signal(
                &expected,
                &FixedIdentity(fake),
                OwnedSignal::Terminate,
                deadline(100),
            )
            .unwrap();
        assert!(matches!(
            result,
            SignalOutcome::Refused(IdentityObservation::Mismatch | IdentityObservation::Unknown)
        ));
    }
    assert_eq!(
        target
            .request_signal(&expected, &MacIdentity, OwnedSignal::Kill, Instant::now())
            .unwrap(),
        SignalOutcome::DeadlineExpired
    );
    let nonce = format!("{:032x}", u128::from(target.pid()) + 1);
    let target_echo = format!("PONG {nonce}\n");
    let target_echo_bytes = target_echo.into_bytes();
    write_all(&mut target, format!("ping {nonce}\n").as_bytes());
    write_all(&mut sentinel, b"ping sentinel\n");
    pump_until(&mut target, |value| {
        value
            .capture()
            .stdout
            .bytes
            .windows(target_echo_bytes.len())
            .any(|part| part == target_echo_bytes.as_slice())
    });
    pump_until(&mut sentinel, |value| {
        value
            .capture()
            .stdout
            .bytes
            .windows(14)
            .any(|part| part == b"PONG sentinel\n")
    });
    cooperative_quit(&mut target);
    cooperative_quit(&mut sentinel);
}

#[test]
fn late_spawn_registration_retains_owned_child_after_deadline() {
    let marker = std::env::temp_dir().join(format!("hf-late-spawn-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let path = marker.to_string_lossy().into_owned();
    let spec = spec("marker", &[&path], 128);
    let end = deadline(300);
    let (pid_tx, pid_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let source = DelayedIdentity {
            observed_pid: pid_tx,
            release: release_rx,
        };
        let child = RetainedOwnedChild::spawn(spec, &source, end).unwrap();
        FixtureGuard(child)
    });
    let pid = pid_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let (seconds, micros, _) = independent_os_probe(pid);
    assert!(
        seconds > 0 && micros < 1_000_000,
        "independent OS identity probe was incomplete"
    );
    let (_timer_tx, timer_rx) = mpsc::channel::<()>();
    assert!(
        timer_rx
            .recv_timeout(end.saturating_duration_since(Instant::now()))
            .is_err(),
        "spawn deadline had not expired at the registration barrier"
    );
    release_tx.send(()).unwrap();
    let mut child = worker.join().unwrap();
    assert_eq!(child.initialization(), SpawnInitialization::DeadlineExpired);
    assert_eq!(child.pid(), pid);
    assert!(child.registered_identity().is_some());
    pump_until(&mut child, |value| {
        value.capture().stdout.bytes.starts_with(b"READY\n")
    });
    assert!(marker.exists(), "fixture did not reach its ready stage");
    cooperative_quit(&mut child);
    std::fs::remove_file(marker).unwrap();
}
