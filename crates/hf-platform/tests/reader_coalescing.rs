#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod reader_support;
use hf_platform::process::{ReaderPolicy, STALL_BOUND};
use reader_support::{consume, lossless, pattern, script, start, wait_for};
use std::time::Duration;

const LONG: Duration = Duration::from_secs(60);
fn long_stall() -> ReaderPolicy {
    ReaderPolicy::with_stall(Duration::from_secs(300))
}
/// Five separated writes: the first four fill the queue, the fifth is left pending.
fn spaced(last: &str) -> String {
    script(&format!(
        "for i in range(4):\n    w(1,b'chunk%d\\n'%i); time.sleep(0.05)\n{last}\n"
    ))
}
/// With no consumer the queue holds four batches and the reader holds the pending one.
fn wait_pending(run: &reader_support::Run) {
    let view = run.reader.view();
    wait_for("a pending batch", LONG, || {
        view.snapshot().stdout.pending > 0
    });
}
fn finish(mut run: reader_support::Run) -> reader_support::Received {
    run.child.close_stdin();
    let got = consume(&mut run, Duration::ZERO, LONG);
    run.reader.join_finished().unwrap();
    got
}

#[test]
fn small_writes_then_eof_never_overflow() {
    let run = start(
        &script("for i in range(20000):\n    w(1,b'x%d\\n'%i)"),
        ReaderPolicy::production(),
    );
    let view = run.reader.view();
    let mut run = run;
    run.child.close_stdin();
    let got = consume(&mut run, Duration::from_micros(200), LONG);
    run.reader.join_finished().unwrap();
    lossless(&view.snapshot());
    let expected: Vec<u8> = (0..20000)
        .flat_map(|i| format!("x{i}\n").into_bytes())
        .collect();
    assert_eq!(got.out, expected);
}

#[test]
fn last_bytes_read_while_queue_full_are_delivered() {
    let run = start(&spaced("w(1,b'last\\n')"), long_stall());
    wait_pending(&run);
    let mut run = run;
    let view = run.reader.view();
    run.child.close_stdin();
    wait_for("end of file", LONG, || view.snapshot().stdout.eof);
    assert_eq!(view.snapshot().stdout.pending, 5);
    assert!(
        !run.reader.is_finished(),
        "pending bytes keep the reader alive"
    );
    let got = finish(run);
    assert_eq!(got.out, b"chunk0\nchunk1\nchunk2\nchunk3\nlast\n");
    lossless(&view.snapshot());
}

#[test]
fn partial_pending_at_eof_is_delivered() {
    let run = start(
        &spaced("w(1,bytes(i%251 for i in range(700)))"),
        long_stall(),
    );
    wait_pending(&run);
    let mut run = run;
    let view = run.reader.view();
    run.child.close_stdin();
    wait_for("end of file", LONG, || view.snapshot().stdout.eof);
    assert_eq!(view.snapshot().stdout.pending, 700);
    let got = finish(run);
    let mut expected = b"chunk0\nchunk1\nchunk2\nchunk3\n".to_vec();
    expected.extend(pattern(700));
    assert_eq!(got.out, expected);
    lossless(&view.snapshot());
}

#[test]
fn exact_cap_pending_at_eof_is_delivered() {
    let run = start(
        &spaced("w(1,bytes(i%251 for i in range(16384)))"),
        long_stall(),
    );
    wait_pending(&run);
    let view = run.reader.view();
    wait_for("the cap", LONG, || view.snapshot().stdout.pending == 16384);
    let mut run = run;
    run.child.close_stdin();
    wait_for("the child to exit", LONG, || {
        run.child.try_wait().unwrap() != hf_domain::process::ProcessExit::Unobserved
    });
    assert!(
        !view.snapshot().stdout.eof,
        "a stream at its cap leaves the poll set"
    );
    let got = finish(run);
    let mut expected = b"chunk0\nchunk1\nchunk2\nchunk3\n".to_vec();
    expected.extend(pattern(16384));
    assert_eq!(got.out, expected);
    lossless(&view.snapshot());
}

#[test]
fn both_streams_interleaved_are_lossless() {
    let writes = "for i in range(5000):\n    w(1,b'o%d\\n'%i)\n    w(2,b'e%d\\n'%i)";
    let mut run = start(&script(writes), ReaderPolicy::production());
    let view = run.reader.view();
    run.child.close_stdin();
    let got = consume(&mut run, Duration::from_micros(300), LONG);
    run.reader.join_finished().unwrap();
    lossless(&view.snapshot());
    let lines = |p: &str| -> Vec<u8> {
        (0..5000)
            .flat_map(|i| format!("{p}{i}\n").into_bytes())
            .collect()
    };
    assert_eq!((got.out, got.err), (lines("o"), lines("e")));
}

#[test]
fn megabyte_burst_with_slow_consumer_is_lossless() {
    let writes = "b=bytes(i%251 for i in range(1<<20))\nfor o in range(0,1<<20,65536):\n    w(1,b[o:o+65536])";
    let mut run = start(&script(writes), ReaderPolicy::production());
    let view = run.reader.view();
    run.child.close_stdin();
    let got = consume(&mut run, Duration::from_millis(15), LONG);
    run.reader.join_finished().unwrap();
    lossless(&view.snapshot());
    assert_eq!(got.out, pattern(1 << 20));
    assert!(got.batches >= 64);
}

#[test]
fn stall_bound_is_two_seconds() {
    assert_eq!(STALL_BOUND, Duration::from_secs(2));
    assert_eq!(ReaderPolicy::production(), ReaderPolicy::production());
}
