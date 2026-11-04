#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::common::*;
use hf_domain::process::{IdentityObservation, ProcessExit};
use hf_platform::process::{
    MacIdentity, OwnedSignal, RetainedOwnedChild, SignalOutcome, WaitOutcome,
};
use std::io;
use std::time::Instant;

#[test]
fn dual_stream_pressure_retains_bounded_raw_bytes_and_marks_overflow() {
    let mut child = spawn("pressure", &["262144"], 1024);
    pump_until(&mut child, |value| {
        value.capture().stdout.overflow && value.capture().stderr.overflow
    });
    let stdout = &child.capture().stdout;
    let stderr = &child.capture().stderr;
    let expected: Vec<u8> = (0..1024).map(|index| (index % 256) as u8).collect();
    assert_eq!(stdout.bytes, expected);
    assert_eq!(
        stderr.bytes,
        expected.iter().map(|byte| 255 - byte).collect::<Vec<_>>()
    );
    write_all(&mut child, b"quit\n");
    child.close_stdin();
    assert_eq!(
        child.wait_until(deadline(3_000), || false).unwrap(),
        WaitOutcome::Exited(ProcessExit::Code(0))
    );
    pump_until(&mut child, |value| {
        value.capture().stdout.eof && value.capture().stderr.eof
    });
    assert!(child.capture().stdout.overflow && child.capture().stderr.overflow);
}

#[test]
fn exit_status_is_observed_independently_of_raw_zombie_and_pipe_eof() {
    let independent_code = independent_exit_code(23);
    assert_eq!(
        independent_code, 23,
        "separate direct-child fixture oracle changed"
    );
    let mut child = spawn("exit", &["23"], 4096);
    let end = deadline(3_000);
    let state = loop {
        let state = independent_process_state_until(child.pid(), end);
        if state.starts_with('Z') || Instant::now() >= end {
            break state;
        }
    };
    assert!(
        state.starts_with('Z'),
        "independent ps did not observe the unreaped child as zombie: {state}"
    );
    assert_eq!(
        child.try_wait().unwrap(),
        ProcessExit::Code(independent_code)
    );
    let identity = child.registered_identity().unwrap().clone();
    assert_eq!(
        child
            .request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(100))
            .unwrap(),
        SignalOutcome::Refused(IdentityObservation::Dead)
    );
    pump_until(&mut child, |value| {
        value.capture().stdout.eof && value.capture().stderr.eof
    });
}

#[test]
fn partial_utf8_is_preserved_as_bytes_across_a_private_rendezvous() {
    let mut child = spawn("partial", &[], 128);
    pump_until(&mut child, |value| !value.capture().stdout.bytes.is_empty());
    assert_eq!(child.capture().stdout.bytes, b"\xe2");
    write_all(&mut child, b"continue\n");
    pump_until(&mut child, |value| {
        value.capture().stdout.bytes.starts_with(b"\xe2\x82\xac")
    });
    assert!(child.capture().stdout.bytes.starts_with(b"\xe2\x82\xac"));
    cooperative_quit(&mut child);
}

#[test]
fn cancelled_observation_does_not_signal_or_discard_the_child() {
    assert_eq!(
        independent_exit_code(0),
        0,
        "separate fixture control did not exit normally"
    );
    let mut child = spawn("hold", &[], 4096);
    pump_until(&mut child, |value| {
        value.capture().stdout.bytes.starts_with(b"READY\n")
    });
    assert_eq!(
        child.wait_until(deadline(500), || true).unwrap(),
        WaitOutcome::Cancelled
    );
    write_all(&mut child, b"ping after-cancel\n");
    pump_until(&mut child, |value| {
        value
            .capture()
            .stdout
            .bytes
            .windows(18)
            .any(|part| part == b"PONG after-cancel\n")
    });
    cooperative_quit(&mut child);
}

#[test]
fn explicit_term_grace_requalification_and_kill_are_bounded() {
    assert_eq!(
        independent_signal_number(),
        9,
        "separate direct-child oracle did not report SIGKILL"
    );
    let mut child = spawn("ignore-term", &[], 4096);
    pump_until(&mut child, |value| {
        value.capture().stdout.bytes.starts_with(b"READY\n")
    });
    let identity = child.registered_identity().unwrap().clone();
    assert_eq!(
        child
            .request_signal(
                &identity,
                &MacIdentity,
                OwnedSignal::Terminate,
                deadline(100)
            )
            .unwrap(),
        SignalOutcome::Sent
    );
    assert_eq!(
        child.wait_until(deadline(100), || false).unwrap(),
        WaitOutcome::DeadlineExpired
    );
    assert_eq!(
        child
            .request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(500))
            .unwrap(),
        SignalOutcome::Sent
    );
    assert_eq!(
        child.wait_until(deadline(2_000), || false).unwrap(),
        WaitOutcome::Exited(ProcessExit::Signal(9))
    );
    pump_until(&mut child, |value| {
        value.capture().stdout.eof && value.capture().stderr.eof
    });
    assert_eq!(
        child.containment(),
        hf_domain::process::Containment::Unknown
    );
}

#[test]
fn dropped_or_pipeholding_descendants_remain_explicitly_unknown_and_cooperatively_cleaned() {
    let mut child = spawn("pipeholder", &[], 4096);
    pump_until(&mut child, |value| {
        value.capture().stdout.bytes.starts_with(b"PIPEHOLDER ")
    });
    let description = String::from_utf8(child.capture().stdout.bytes.clone()).unwrap();
    let fields: Vec<&str> = description.split_whitespace().collect();
    assert_eq!(fields.len(), 3);
    let descendant_pid: u32 = fields[1].parse().unwrap();
    let reported_group: u32 = fields[2].parse().unwrap();
    assert_eq!(independent_group(descendant_pid), reported_group);
    assert_eq!(
        reported_group, descendant_pid,
        "fixture descendant did not escape its parent's initial group"
    );
    assert_eq!(
        child.wait_until(deadline(2_000), || false).unwrap(),
        WaitOutcome::Exited(ProcessExit::Code(0))
    );
    child.pump(deadline(200), 4096).unwrap();
    assert!(!child.capture().stdout.eof || !child.capture().stderr.eof);
    assert_eq!(
        child.containment(),
        hf_domain::process::Containment::Unknown
    );
    // The descendant inherited this private stdin channel and exits cooperatively.
    write_all(&mut child, b"quit\n");
    child.close_stdin();
    pump_until(&mut child, |value| {
        value.capture().stdout.eof && value.capture().stderr.eof
    });
    let end = deadline(3_000);
    let mut state = independent_process_state_until(descendant_pid, end);
    while !state.is_empty() && Instant::now() < end {
        state = independent_process_state_until(descendant_pid, end);
    }
    assert!(
        state.is_empty(),
        "escaped fixture descendant survived cooperative channel cleanup"
    );
}

#[test]
fn expired_spawn_does_not_execute_fixture() {
    let marker = std::env::temp_dir().join(format!("hf-expired-spawn-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let path = marker.to_string_lossy().into_owned();
    let spec = spec("marker", &[&path], 128);
    let result = RetainedOwnedChild::spawn(spec, &MacIdentity, Instant::now());
    assert_eq!(result.err().unwrap().kind(), io::ErrorKind::TimedOut);
    assert!(
        !marker.exists(),
        "expired spawn executed its fixture side effect"
    );
}
