#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_domain::process::ProcessIdentity;
use hf_platform::process::{
    MacIdentity, OwnedSignal, PrivateStorageIo, RetainedOwnedChild, SignalOutcome, SpawnSpec,
    WaitOutcome,
};
use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

const HELPER: &str = r#"
import os, sys
mode = sys.argv[1]
def read_exact(size):
    data = bytearray()
    while len(data) < size:
        chunk = os.read(0, size - len(data))
        if not chunk:
            return bytes(data)
        data.extend(chunk)
    return bytes(data)
def read_frame():
    prefix = read_exact(4)
    if len(prefix) != 4:
        return b''
    size = int.from_bytes(prefix, 'big')
    body = read_exact(size)
    return prefix + body if len(body) == size else b''
if mode == 'stall':
    while os.read(0, 4096):
        pass
else:
    if not read_frame():
        sys.exit(0)
    if mode == 'fragment':
        response = (78).to_bytes(4, 'big') + bytes(range(78))
        os.write(1, response[:2])
        os.write(2, b'P')
        if read_frame():
            for byte in response[2:]:
                os.write(1, bytes((byte,)))
            os.write(2, b'E' * 8192)
    elif mode == 'oversized':
        os.write(1, (24573).to_bytes(4, 'big'))
        os.write(1, b'X' * 8192)
    elif mode == 'truncated':
        os.write(1, (78).to_bytes(4, 'big') + b'X')
os.close(1)
os.close(2)
"#;

fn deadline(ms: u64) -> Instant {
    Instant::now() + Duration::from_millis(ms)
}
fn frame(body: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(body);
    frame
}
fn request_frame(size: usize) -> Vec<u8> {
    frame(&vec![0x5a; size])
}

struct Worker {
    child: RetainedOwnedChild,
    io: Option<PrivateStorageIo>,
    reaped: bool,
}
impl Worker {
    fn new(mode: &str) -> Self {
        let child = RetainedOwnedChild::spawn(
            SpawnSpec {
                executable: PathBuf::from("/usr/bin/python3"),
                args: vec![
                    "-I".into(),
                    "-S".into(),
                    "-c".into(),
                    HELPER.into(),
                    OsString::from(mode),
                ],
                cwd: fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
                environment: Vec::new(),
                capture_limit: 0,
            },
            &MacIdentity,
            deadline(2_000),
        )
        .unwrap();
        let identity = child
            .registered_identity()
            .cloned()
            .expect("fixed helper identity must qualify");
        let mut child = child;
        let io = child.take_private_storage_io(&identity).unwrap();
        Self {
            child,
            io: Some(io),
            reaped: false,
        }
    }
    fn transport(&mut self) -> &mut PrivateStorageIo {
        self.io.as_mut().unwrap()
    }
    fn transport_ref(&self) -> &PrivateStorageIo {
        self.io.as_ref().unwrap()
    }
    fn finish(&mut self) -> ProcessIdentity {
        if let Some(io) = self.io.as_mut() {
            io.close_input();
        }
        self.io.take();
        self.child.close_stdin();
        if !matches!(
            self.child
                .wait_exit_until(deadline(1_000), || false)
                .unwrap(),
            WaitOutcome::Exited(_)
        ) {
            let identity = self
                .child
                .registered_identity()
                .cloned()
                .expect("owned helper identity lost");
            assert_eq!(
                self.child
                    .request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(500))
                    .unwrap(),
                SignalOutcome::Sent
            );
            assert!(matches!(
                self.child
                    .wait_exit_until(deadline(1_000), || false)
                    .unwrap(),
                WaitOutcome::Exited(_)
            ));
        }
        self.reaped = true;
        self.child.registered_identity().unwrap().clone()
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.finish();
        }
    }
}

fn step_until(worker: &mut Worker, timeout_ms: u64, condition: impl Fn(&Worker) -> bool) {
    let until = deadline(timeout_ms);
    while !condition(worker) {
        assert!(
            Instant::now() < until,
            "storage transport condition did not settle"
        );
        worker.transport().step(until).unwrap_or_else(|error| {
            panic!("storage transport step failed before expected observation: {error}")
        });
    }
}

#[test]
fn transport_moves_full_frames_and_reassembles_fragmented_rx_while_bounding_stderr() {
    let mut worker = Worker::new("fragment");
    let input = request_frame(8192);
    worker.transport().queue_frame(input.clone()).unwrap();
    worker.transport().step(deadline(500)).unwrap();
    assert!(worker.transport().sent_offset() > 0);
    assert!(worker.transport().sent_offset() <= 4096);
    assert!(!worker.transport().send_complete());
    assert!(worker.transport().queue_frame(request_frame(78)).is_err());

    step_until(&mut worker, 2_000, |worker| {
        worker.transport_ref().stderr().retained == b"P"
    });
    assert!(worker.transport().send_complete());
    assert!(
        worker.transport().take_frame().is_none(),
        "partial prefix became a frame"
    );
    worker.transport().queue_frame(request_frame(78)).unwrap();
    let until = deadline(2_000);
    let received = loop {
        if let Some(frame) = worker.transport().take_frame() {
            break frame;
        }
        assert!(Instant::now() < until, "fragmented full reply missing");
        worker.transport().step(until).unwrap();
    };
    assert_eq!(
        received,
        frame(&(0..78).map(|value| value as u8).collect::<Vec<_>>())
    );
    step_until(&mut worker, 2_000, |worker| {
        worker.transport_ref().channels_complete()
    });
    assert_eq!(worker.transport().stderr().retained.len(), 4096);
    assert_eq!(worker.transport().stderr().discarded, 4097);
    assert!(worker.transport().is_complete());
    worker.finish();
}

#[test]
fn transport_rejects_oversized_and_truncated_rx_then_drains_to_eof() {
    let mut oversized = Worker::new("oversized");
    oversized
        .transport()
        .queue_frame(request_frame(78))
        .unwrap();
    let until = deadline(2_000);
    let mut rejected = false;
    while !oversized.transport().channels_complete() {
        assert!(Instant::now() < until, "oversized stream did not reach EOF");
        rejected |= oversized.transport().step(until).is_err();
    }
    assert!(rejected);
    assert!(oversized.transport().take_frame().is_none());
    assert!(!oversized.transport().is_complete());
    assert!(
        oversized
            .transport()
            .queue_frame(request_frame(78))
            .is_err()
    );
    oversized.finish();

    let mut truncated = Worker::new("truncated");
    truncated
        .transport()
        .queue_frame(request_frame(78))
        .unwrap();
    let until = deadline(2_000);
    let mut rejected = false;
    while !truncated.transport().channels_complete() {
        assert!(Instant::now() < until, "truncated stream did not reach EOF");
        rejected |= truncated.transport().step(until).is_err();
    }
    assert!(rejected);
    assert!(truncated.transport().take_frame().is_none());
    truncated.finish();
}

#[test]
fn stalled_writer_does_not_block_a_bounded_owner_step_and_partial_tx_is_abandoned() {
    let mut worker = Worker::new("stall");
    let start = Instant::now();
    worker
        .transport()
        .step(start + Duration::from_millis(500))
        .unwrap();
    assert!(
        start.elapsed() < Duration::from_millis(250),
        "owner step blocked on a stalled writer"
    );
    assert!(!worker.transport().channels_complete());
    let full = request_frame(8192);
    worker.transport().queue_frame(full).unwrap();
    worker.transport().step(deadline(500)).unwrap();
    assert!(worker.transport().sent_offset() > 0);
    assert!(!worker.transport().send_complete());
    worker.transport().close_input();
    assert!(worker.transport().tx_abandoned());
    assert!(!worker.transport().send_complete());
    worker.finish();
}
