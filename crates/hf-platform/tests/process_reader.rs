#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use hf_application::process::IdentityPort;
use hf_domain::{
    ownership::{Digest, Id, OwnerClaim, Phase, PreparedRequest, Snapshot},
    process::{ProcessExit, ProcessIdentity},
};
use hf_platform::process::{
    CaptureOrigin, MacIdentity, OwnedSignal, RetainedOwnedChild, SignalOutcome, SpawnSpec,
    WaitOutcome,
};
use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

const HELPER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/reader_writer.py"
);

fn deadline(ms: u64) -> Instant {
    Instant::now() + Duration::from_millis(ms)
}

fn id(digit: char) -> Id {
    Id::new(digit.to_string().repeat(32)).unwrap()
}

struct ChildGuard(RetainedOwnedChild);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.0.close_stdin();
        if matches!(
            self.0.wait_exit_until(deadline(1_000), || false),
            Ok(WaitOutcome::Exited(_))
        ) {
            return;
        }
        let Some(identity) = self.0.registered_identity().cloned() else {
            std::process::abort();
        };
        if self
            .0
            .request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(500))
            .ok()
            != Some(SignalOutcome::Sent)
            || !matches!(
                self.0.wait_exit_until(deadline(1_000), || false),
                Ok(WaitOutcome::Exited(_))
            )
        {
            std::process::abort();
        }
    }
}

fn request() -> PreparedRequest {
    PreparedRequest {
        operation: id('1'),
        session: id('2'),
        reservation: id('3'),
        attempt: id('4'),
        generation: id('5'),
        digest: Digest::new("6".repeat(64)).unwrap(),
        payload: Vec::new(),
    }
}

fn origin(child: &ProcessIdentity) -> CaptureOrigin {
    let owner = OwnerClaim {
        owner: id('7'),
        nonce: id('8'),
        process: MacIdentity.identity(std::process::id()).unwrap(),
    };
    CaptureOrigin::from_spawn_observed(&Snapshot {
        request: request(),
        phase: Phase::SpawnObserved,
        revision: 9,
        cancelled: false,
        owner: Some(owner),
        child: Some(child.clone()),
    })
    .unwrap()
}

#[test]
fn actual_reader_drains_both_pipes_while_consumer_is_held_and_keeps_exact_loss_prefix() {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let scratch = std::env::temp_dir().join(format!(
        "hf-reader-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir(&scratch).unwrap();
    let witness = scratch.join("producer.finished");
    let child = RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: PathBuf::from("/usr/bin/python3"),
            args: vec![
                OsString::from("-I"),
                OsString::from("-S"),
                OsString::from(HELPER),
                witness.as_os_str().to_owned(),
            ],
            cwd: fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
            environment: Vec::new(),
            capture_limit: 1,
        },
        &MacIdentity,
        deadline(3_000),
    )
    .unwrap();
    let mut child = ChildGuard(child);
    let identity = child.0.registered_identity().unwrap().clone();
    let mut reader = child
        .0
        .start_private_fixture_capture_reader(origin(&identity), deadline(8_000))
        .unwrap();
    verify_full_drain(&mut child, &mut reader, &witness, &scratch);
}

fn verify_full_drain(
    child: &mut ChildGuard,
    reader: &mut hf_platform::process::PrivateFixtureCaptureReader,
    witness: &std::path::Path,
    scratch: &std::path::Path,
) {
    let view = reader.view();
    let until = deadline(6_000);
    let progress = wait_for_producer(&view, witness, until);
    assert!(witness.exists(), "producer did not finish both streams");
    assert_eq!(fs::read(witness).unwrap(), b"complete");
    assert_eq!(progress.stdout.read, 262_144);
    assert_eq!(progress.stderr.read, 262_144);
    assert!(progress.discard, "full data queue did not latch discard");
    assert!(progress.stdout.discarded > 0 && progress.stderr.discarded > 0);
    assert_eq!(
        progress.stdout.read,
        progress.stdout.enqueued + progress.stdout.discarded
    );
    assert_eq!(
        progress.stderr.read,
        progress.stderr.enqueued + progress.stderr.discarded
    );

    child.0.close_stdin();
    assert!(matches!(
        child.0.wait_exit_until(deadline(3_000), || false).unwrap(),
        WaitOutcome::Exited(ProcessExit::Code(0))
    ));
    let final_value = loop {
        if let Some(value) = view.final_observation() {
            break value;
        }
        assert!(Instant::now() < until, "reader final observation timed out");
        std::thread::yield_now();
    };
    assert!(final_value.progress.stdout.eof && final_value.progress.stderr.eof);
    assert!(!final_value.stopped && !final_value.limited);
    assert_eq!(reader.join_finished().unwrap(), final_value);

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    while let Ok(batch) = reader.try_recv() {
        assert_eq!(batch.origin, *reader.origin());
        stdout.extend(batch.stdout);
        stderr.extend(batch.stderr);
    }
    assert_eq!(stdout.len() as u64, final_value.progress.stdout.enqueued);
    assert_eq!(stderr.len() as u64, final_value.progress.stderr.enqueued);
    assert_eq!(stdout, pattern(262_144, 17, 3)[..stdout.len()]);
    assert_eq!(stderr, pattern(262_144, 29, 7)[..stderr.len()]);

    fs::remove_dir_all(scratch).unwrap();
}

fn wait_for_producer(
    view: &hf_platform::process::ReaderView,
    witness: &std::path::Path,
    until: Instant,
) -> hf_platform::process::ReaderProgress {
    loop {
        let progress = view.snapshot();
        if witness.exists() && progress.stdout.read == 262_144 && progress.stderr.read == 262_144 {
            return progress;
        }
        assert!(Instant::now() < until, "producer drain deadline");
        std::thread::yield_now();
    }
}

fn pattern(length: usize, multiplier: usize, addend: usize) -> Vec<u8> {
    (0..length)
        .map(|index| ((index * multiplier + addend) % 256) as u8)
        .collect()
}
