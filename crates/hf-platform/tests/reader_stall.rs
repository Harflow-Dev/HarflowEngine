#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod reader_support;
use hf_platform::process::{LossCause, ReaderPolicy};
use reader_support::{consume, lossless, script, start, wait_for};
use std::time::Duration;

const LONG: Duration = Duration::from_secs(60);
/// Four batches fill the queue, the fifth stays pending; the child then waits on stdin.
const FILL: &str = "for i in range(5):\n    w(1,b'chunk%d\\n'%i); time.sleep(0.05)";

#[test]
fn stall_with_partial_pending_overflows() {
    let mut run = start(
        &script(FILL),
        ReaderPolicy::with_stall(Duration::from_millis(300)),
    );
    let view = run.reader.view();
    wait_for("the stall", LONG, || view.snapshot().overflowed);
    let progress = view.snapshot();
    assert!(progress.discard);
    assert_eq!(progress.overflow_cause, Some(LossCause::Stall));
    assert_eq!(
        progress.stdout.pending, 0,
        "pending bytes count as discarded"
    );
    assert_eq!(progress.stdout.discarded, 7, "the pending batch");
    assert_eq!(
        progress.stdout.read,
        progress.stdout.enqueued + progress.stdout.discarded
    );
    run.child.close_stdin();
    wait_for("the reader", LONG, || run.reader.is_finished());
    let last = run.reader.join_finished().unwrap().progress;
    assert_eq!(last.overflow_cause, Some(LossCause::Stall));
    assert_eq!(
        last.stdout.read,
        last.stdout.enqueued + last.stdout.discarded
    );
}

#[test]
fn requested_discard_releases_blocked_reader() {
    let writes = "b=bytes(i%251 for i in range(1<<20))\nfor o in range(0,1<<20,65536):\n    w(1,b[o:o+65536])";
    let mut run = start(
        &script(writes),
        ReaderPolicy::with_stall(Duration::from_secs(300)),
    );
    let view = run.reader.view();
    wait_for("a blocked reader", LONG, || {
        let p = view.snapshot();
        p.stdout.pending == 16384
    });
    std::thread::sleep(Duration::from_millis(200));
    assert!(!run.reader.is_finished());
    run.reader.discard_capture();
    run.child.close_stdin();
    wait_for("the reader", LONG, || run.reader.is_finished());
    let last = run.reader.join_finished().unwrap().progress;
    assert!(last.discard && !last.overflowed);
    assert_eq!(last.overflow_cause, None, "a request is never a loss cause");
    assert_eq!(last.stdout.pending, 0);
    assert_eq!(last.stdout.read, 1 << 20);
    assert_eq!(
        last.stdout.read,
        last.stdout.enqueued + last.stdout.discarded
    );
    assert!(last.stdout.discarded > 0);
}

#[test]
fn held_descendant_keeps_pending_until_real_eof() {
    let writes = format!("{FILL}\nimport subprocess\nsubprocess.Popen(['/bin/sleep','2'])");
    let mut run = start(
        &script(&writes),
        ReaderPolicy::with_stall(Duration::from_secs(300)),
    );
    let view = run.reader.view();
    run.child.close_stdin();
    wait_for("the child to exit", LONG, || {
        run.child.try_wait().unwrap() != hf_domain::process::ProcessExit::Unobserved
    });
    std::thread::sleep(Duration::from_millis(300));
    let progress = view.snapshot();
    assert!(!progress.stdout.eof, "the descendant still holds the pipe");
    assert!(progress.stdout.pending > 0 && !run.reader.is_finished());
    let got = consume(&mut run, Duration::ZERO, LONG);
    run.reader.join_finished().unwrap();
    assert_eq!(got.out, b"chunk0\nchunk1\nchunk2\nchunk3\nchunk4\n");
    lossless(&view.snapshot());
}
