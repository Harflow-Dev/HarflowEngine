#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use hf_application::process::IdentityPort;
use hf_domain::process::ProcessExit;
use hf_platform::process::{MacIdentity, WaitOutcome};
use std::time::{Duration, Instant};
#[path = "reader_boundaries/support.rs"]
mod reader_boundaries_support;
use reader_boundaries_support::{LIMIT_HELPER, child, child_with, deadline, origin, wait_witness};

#[test]
fn explicit_stop_while_both_child_pipes_remain_open_is_unknown_tail_not_eof() {
    let mut owner = child();
    let identity = owner.0.registered_identity().unwrap().clone();
    let mut reader = owner
        .0
        .start_private_fixture_capture_reader(origin(&identity), deadline(8_000))
        .unwrap();
    let view = reader.view();
    wait_witness(&owner.1.join("written"));
    let until = deadline(6_000);
    let mut progress = view.snapshot();
    while (progress.stdout.read == 0 || progress.stderr.read == 0) && Instant::now() < until {
        std::thread::yield_now();
        progress = view.snapshot();
    }
    assert!(progress.stdout.read > 0 && progress.stderr.read > 0);
    assert!(!progress.stdout.eof && !progress.stderr.eof);
    reader.request_stop();
    let observed = reader.settle(deadline(900)).unwrap();
    assert!(observed.stopped);
    assert!(!observed.progress.stdout.eof && !observed.progress.stderr.eof);
    assert!(observed.progress.stdout.read > 0 && observed.progress.stderr.read > 0);
    assert_eq!(reader.join_finished().unwrap(), observed);
    assert_eq!(view.final_observation(), Some(observed.clone()));
    assert_eq!(view.final_observation(), Some(observed));
    owner.0.close_stdin();
    drop(reader);
}

#[test]
fn finite_reader_deadline_yields_limited_unknown_unread_tail() {
    let mut owner = child();
    let identity = owner.0.registered_identity().unwrap().clone();
    let started = Instant::now();
    let mut reader = owner
        .0
        .start_private_fixture_capture_reader(origin(&identity), deadline(120))
        .unwrap();
    let observed = reader.settle(deadline(800)).unwrap();
    assert!(observed.limited);
    assert!(!observed.stopped);
    assert!(!observed.progress.stdout.eof && !observed.progress.stderr.eof);
    assert!(
        started.elapsed() < Duration::from_millis(1_500),
        "reader exceeded finite deadline"
    );
    assert_eq!(reader.join_finished().unwrap(), observed);
}

#[test]
fn per_stream_byte_limit_is_limited_unknown_tail_not_eof_or_exact_loss() {
    let mut owner = child_with(LIMIT_HELPER);
    let identity = owner.0.registered_identity().unwrap().clone();
    let mut reader = owner
        .0
        .start_private_fixture_capture_reader(origin(&identity), deadline(8_000))
        .unwrap();
    let until = deadline(7_000);
    while !reader.is_finished() && Instant::now() < until {
        std::thread::yield_now();
    }
    assert!(
        reader.is_finished(),
        "fixed helper stream-cap reader deadline"
    );
    let observed = reader.join_finished().unwrap();
    assert!(observed.limited && !observed.stopped);
    for stream in [&observed.progress.stdout, &observed.progress.stderr] {
        assert!(stream.read > 0 && stream.read <= 8 * 1024 * 1024);
        assert_eq!(stream.read, stream.enqueued + stream.discarded);
        assert!(!stream.eof, "byte limit was misreported as EOF");
        assert!(stream.error.is_none());
    }
    assert!(
        observed.progress.stdout.read == 8 * 1024 * 1024
            || observed.progress.stderr.read == 8 * 1024 * 1024
    );
}

#[test]
fn read_only_observer_timeout_and_drop_cannot_stop_reader_or_replace_child_waiter() {
    let mut owner = child();
    let identity = owner.0.registered_identity().unwrap().clone();
    let mut reader = owner
        .0
        .start_private_fixture_capture_reader(origin(&identity), deadline(8_000))
        .unwrap();
    let view = reader.view();
    let observer_until = deadline(25);
    let before = loop {
        let snapshot = view.snapshot();
        if Instant::now() >= observer_until {
            break snapshot;
        }
        std::thread::yield_now();
    };
    assert!(
        !reader.is_finished(),
        "observer timeout stopped a live reader"
    );
    assert_eq!(
        owner.0.exit(),
        ProcessExit::Unobserved,
        "observer replaced the sole child waiter"
    );
    drop(view);
    assert!(!reader.is_finished(), "dropping observer stopped reader");
    owner.0.close_stdin();
    assert!(matches!(
        owner.0.wait_exit_until(deadline(2_000), || false).unwrap(),
        WaitOutcome::Exited(ProcessExit::Code(0))
    ));
    let final_value = reader.settle(deadline(2_000)).unwrap();
    assert!(!final_value.stopped && !final_value.limited);
    assert!(final_value.progress.stdout.eof && final_value.progress.stderr.eof);
    assert!(final_value.progress.stdout.read >= before.stdout.read);
    assert!(final_value.progress.stderr.read >= before.stderr.read);
}

#[test]
fn handoff_rejects_mismatched_child_and_prebuffered_capture_without_consuming_pipes() {
    let mut owner = child();
    let registered = owner.0.registered_identity().unwrap().clone();
    let wrong = MacIdentity.identity(std::process::id()).unwrap();
    assert!(
        owner
            .0
            .start_private_fixture_capture_reader(origin(&wrong), deadline(2_000))
            .is_err()
    );
    let mut reader = owner
        .0
        .start_private_fixture_capture_reader(origin(&registered), deadline(2_000))
        .unwrap();
    assert!(
        owner
            .0
            .start_private_fixture_capture_reader(origin(&registered), deadline(2_000))
            .is_err()
    );
    reader.request_stop();
    let _ = reader.settle(deadline(900)).unwrap();
    drop(reader);
    let mut buffered = child();
    let until = deadline(6_000);
    while buffered.0.capture().stdout.bytes.is_empty() && Instant::now() < until {
        buffered.0.pump(deadline(20), 65_536).unwrap();
        std::thread::yield_now();
    }
    assert!(
        !buffered.0.capture().stdout.bytes.is_empty(),
        "fixed helper produced no pre-handoff bytes"
    );
    assert!(
        buffered.0.capture().stdout.overflow,
        "capture overflow state was not retained"
    );
    let buffered_identity = buffered.0.registered_identity().unwrap().clone();
    assert!(
        buffered
            .0
            .start_private_fixture_capture_reader(origin(&buffered_identity), deadline(2_000))
            .is_err()
    );

    let mut eof = child();
    let eof_identity = eof.0.registered_identity().unwrap().clone();
    let witness_until = deadline(6_000);
    while !eof.1.join("written").exists() && Instant::now() < witness_until {
        eof.0.pump(deadline(20), 65_536).unwrap();
        std::thread::yield_now();
    }
    assert!(
        eof.1.join("written").exists(),
        "fixed helper did not finish writes"
    );
    eof.0.close_stdin();
    assert!(matches!(
        eof.0.wait_exit_until(deadline(2_000), || false).unwrap(),
        WaitOutcome::Exited(ProcessExit::Code(0))
    ));
    let until = deadline(2_000);
    while (!eof.0.capture().stdout.eof || !eof.0.capture().stderr.eof) && Instant::now() < until {
        eof.0.pump(deadline(20), 65_536).unwrap();
        std::thread::yield_now();
    }
    assert!(eof.0.capture().stdout.eof && eof.0.capture().stderr.eof);
    assert!(
        eof.0
            .start_private_fixture_capture_reader(origin(&eof_identity), deadline(2_000))
            .is_err()
    );
}
