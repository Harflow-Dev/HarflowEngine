use super::reader::{
    CaptureBatch, CaptureOrigin, PrivateFixtureCaptureReader, ReaderFinal, ReaderProgress,
    ReaderStreamStats, Shared,
};
use nix::poll::{PollFd, PollFlags, poll};
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::AsFd;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
    mpsc::{SyncSender, sync_channel},
};
use std::time::{Duration, Instant};
const STREAM_LIMIT: u64 = 8 * 1024 * 1024;
pub(super) fn start(
    origin: CaptureOrigin,
    out: File,
    err: File,
    deadline: Instant,
) -> io::Result<PrivateFixtureCaptureReader> {
    let shared = Arc::new(Shared {
        progress: Mutex::new(ReaderProgress::default()),
        final_value: OnceLock::new(),
        stop: AtomicBool::new(false),
        discard: AtomicBool::new(false),
    });
    let (sender, receiver) = sync_channel(4);
    let work = shared.clone();
    let tag = origin.clone();
    let deadline = deadline.min(Instant::now() + Duration::from_secs(8));
    let worker = std::thread::Builder::new()
        .name("fixture-pipe-reader".into())
        .spawn(move || run(tag, out, err, deadline, sender, work))?;
    Ok(PrivateFixtureCaptureReader {
        origin,
        shared,
        receiver,
        worker: Some(worker),
        join_failed: false,
    })
}
fn read_one(pipe: &mut File, stats: &mut ReaderStreamStats) -> Vec<u8> {
    if stats.eof || stats.error.is_some() || stats.read >= STREAM_LIMIT {
        return Vec::new();
    }
    let mut bytes = vec![0; (STREAM_LIMIT - stats.read).min(16384) as usize];
    match pipe.read(&mut bytes) {
        Ok(0) => {
            stats.eof = true;
            bytes.clear();
        }
        Ok(count) => {
            bytes.truncate(count);
            stats.read += count as u64;
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            bytes.clear()
        }
        Err(error) => {
            stats.error = Some(error.to_string());
            bytes.clear();
        }
    }
    bytes
}
fn enqueue(
    batch: CaptureBatch,
    progress: &mut ReaderProgress,
    sender: &SyncSender<CaptureBatch>,
    shared: &Shared,
) {
    let out = batch.stdout.len() as u64;
    let err = batch.stderr.len() as u64;
    progress.discard |= shared.discard.load(Ordering::Acquire);
    if out + err == 0 {
        return;
    }
    if !progress.discard && sender.try_send(batch).is_err() {
        progress.discard = true;
    }
    if progress.discard {
        progress.stdout.discarded += out;
        progress.stderr.discarded += err;
    } else {
        progress.stdout.enqueued += out;
        progress.stderr.enqueued += err;
    }
}
fn run(
    origin: CaptureOrigin,
    mut out: File,
    mut err: File,
    deadline: Instant,
    sender: SyncSender<CaptureBatch>,
    shared: Arc<Shared>,
) -> ReaderFinal {
    let mut progress = ReaderProgress::default();
    let mut rounds = 0;
    while rounds < 4096 && Instant::now() < deadline && !shared.stop.load(Ordering::Acquire) {
        if finished(&progress.stdout) && finished(&progress.stderr) {
            break;
        }
        if progress.stdout.read >= STREAM_LIMIT || progress.stderr.read >= STREAM_LIMIT {
            break;
        }
        rounds += 1;
        if let Err(error) = poll_pair(&out, &err, &progress, deadline) {
            progress
                .stdout
                .error
                .get_or_insert_with(|| error.to_string());
            progress
                .stderr
                .error
                .get_or_insert_with(|| error.to_string());
            break;
        }
        let batch = CaptureBatch {
            origin: origin.clone(),
            stdout: read_one(&mut out, &mut progress.stdout),
            stderr: read_one(&mut err, &mut progress.stderr),
        };
        enqueue(batch, &mut progress, &sender, &shared);
        shared.publish(&progress);
    }
    let stopped = shared.stop.load(Ordering::Acquire);
    let limited = !stopped && !(finished(&progress.stdout) && finished(&progress.stderr));
    let final_value = ReaderFinal {
        progress,
        stopped,
        limited,
    };
    shared.publish(&final_value.progress);
    let _ = shared.final_value.set(final_value.clone());
    final_value
}
fn finished(stats: &ReaderStreamStats) -> bool {
    stats.eof || stats.error.is_some()
}
fn poll_pair(
    out: &File,
    err: &File,
    progress: &ReaderProgress,
    deadline: Instant,
) -> io::Result<()> {
    let mut descriptors = Vec::with_capacity(2);
    if !finished(&progress.stdout) {
        descriptors.push(PollFd::new(out.as_fd(), PollFlags::POLLIN));
    }
    if !finished(&progress.stderr) {
        descriptors.push(PollFd::new(err.as_fd(), PollFlags::POLLIN));
    }
    let millis = deadline
        .saturating_duration_since(Instant::now())
        .as_millis()
        .min(20) as u16;
    match poll(&mut descriptors, millis) {
        Ok(_) | Err(nix::errno::Errno::EINTR) => Ok(()),
        Err(error) => Err(error.into()),
    }
}
