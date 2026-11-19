//! Own process: the CPU of a stalled reader is measured over the whole test process.
#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod reader_support;
use hf_platform::process::ReaderPolicy;
use reader_support::{script, start, wait_for};
use std::{process::Command, time::Duration};

/// Process CPU seconds as `ps` reports them (hundredths of a second).
fn cpu_seconds() -> f64 {
    let out = Command::new("/bin/ps")
        .args(["-o", "cputime=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    text.trim().split(':').fold(0.0, |total, part| {
        total * 60.0 + part.parse::<f64>().unwrap()
    })
}

#[test]
fn stalled_reader_cpu_stays_bounded() {
    let writes = "b=bytes(i%251 for i in range(1<<20))\nfor o in range(0,1<<20,65536):\n    w(1,b[o:o+65536])";
    let mut run = start(
        &script(writes),
        ReaderPolicy::with_stall(Duration::from_secs(300)),
    );
    let view = run.reader.view();
    wait_for("a blocked reader", Duration::from_secs(60), || {
        view.snapshot().stdout.pending == 16384
    });
    std::thread::sleep(Duration::from_millis(500));
    let before = cpu_seconds();
    std::thread::sleep(Duration::from_secs(1));
    let spent = cpu_seconds() - before;
    assert!(
        spent < 0.1,
        "a stalled reader spent {spent} s of CPU in 1 s"
    );
    run.reader.discard_capture();
    run.child.close_stdin();
    wait_for("the reader", Duration::from_secs(60), || {
        run.reader.is_finished()
    });
}
