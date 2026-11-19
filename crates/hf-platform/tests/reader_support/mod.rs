//! A real child that writes a scripted byte stream, and a consumer of the capture reader.
#![allow(dead_code)]
use hf_application::process::IdentityPort;
use hf_domain::ownership::{Fence, Id, OwnerClaim};
use hf_platform::process::{
    CaptureOrigin, CaptureReader, MacIdentity, ReaderPolicy, ReaderProgress, RetainedOwnedChild,
    SpawnSpec,
};
use std::{
    ffi::OsString,
    time::{Duration, Instant},
};

pub fn id(digit: char) -> Id {
    Id::new(digit.to_string().repeat(32)).unwrap()
}
pub struct Run {
    pub child: RetainedOwnedChild,
    pub reader: CaptureReader,
}
/// Spawns `python3 -I -S -c script`: the script reads stdin to its end last, so closing the
/// child's stdin lets it exit.
pub fn start(script: &str, policy: ReaderPolicy) -> Run {
    let spec = SpawnSpec {
        executable: "/usr/bin/python3".into(),
        cwd: std::env::current_dir().unwrap(),
        args: ["-I", "-S", "-c", script].map(OsString::from).to_vec(),
        environment: Vec::new(),
        capture_limit: 0,
    };
    let end = Instant::now() + Duration::from_secs(30);
    let mut child = RetainedOwnedChild::spawn(spec, &MacIdentity, end).unwrap();
    let identity = child.registered_identity().cloned().unwrap();
    let origin = CaptureOrigin {
        child: identity,
        fence: Fence {
            operation: id('1'),
            generation: id('2'),
            attempt: id('3'),
            owner: OwnerClaim {
                owner: id('1'),
                nonce: id('4'),
                process: MacIdentity.identity(std::process::id()).unwrap(),
            },
            revision: 2,
        },
    };
    let reader = child.start_capture_reader(origin, policy).unwrap();
    Run { child, reader }
}
pub fn wait_for(what: &str, within: Duration, mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + within;
    while !ready() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}
/// Everything the consumer received, per stream.
#[derive(Default)]
pub struct Received {
    pub out: Vec<u8>,
    pub err: Vec<u8>,
    pub batches: usize,
}
/// Takes batches until the reader has finished and the queue is empty, pausing `pause`
/// after each batch.
pub fn consume(run: &mut Run, pause: Duration, within: Duration) -> Received {
    let mut got = Received::default();
    let end = Instant::now() + within;
    loop {
        match run.reader.try_recv() {
            Ok(batch) => {
                got.out.extend(batch.stdout);
                got.err.extend(batch.stderr);
                got.batches += 1;
                std::thread::sleep(pause);
            }
            Err(_) if run.reader.is_finished() => {
                if let Ok(batch) = run.reader.try_recv() {
                    got.out.extend(batch.stdout);
                    got.err.extend(batch.stderr);
                    got.batches += 1;
                    continue;
                }
                return got;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(1)),
        }
        assert!(Instant::now() < end, "consumer timed out");
    }
}
/// The deterministic payload of `length` bytes that scripts and assertions agree on.
pub fn pattern(length: usize) -> Vec<u8> {
    (0..length).map(|i| (i % 251) as u8).collect()
}
pub fn lossless(progress: &ReaderProgress) {
    assert!(!progress.overflowed && !progress.discard, "{progress:?}");
    assert_eq!(progress.overflow_cause, None);
    for stats in [&progress.stdout, &progress.stderr] {
        assert_eq!((stats.discarded, stats.pending), (0, 0), "{progress:?}");
        assert_eq!(stats.read, stats.enqueued);
        assert!(stats.eof && stats.error.is_none());
    }
}
/// Python source that writes `PAYLOAD` pieces separated by `gap` seconds, then waits for
/// its stdin to close.
pub fn script(writes: &str) -> String {
    format!(
        "import os,sys,time\ndef w(fd,b): \n    while b:\n        n=os.write(fd,b); b=b[n:]\n{writes}\nsys.stdin.read()\n"
    )
}
