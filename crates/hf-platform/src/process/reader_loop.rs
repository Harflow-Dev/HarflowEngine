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
    start_policy(
        origin,
        out,
        err,
        Some(deadline.min(Instant::now() + Duration::from_secs(8))),
    )
}
pub(super) fn start_production(
    origin: CaptureOrigin,
    out: File,
    err: File,
) -> io::Result<PrivateFixtureCaptureReader> {
    start_policy(origin, out, err, None)
}
fn start_policy(
    origin: CaptureOrigin,
    out: File,
    err: File,
    deadline: Option<Instant>,
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
fn read_one(pipe: &mut File, stats: &mut ReaderStreamStats, limit: Option<u64>) -> Vec<u8> {
    if stats.eof || stats.error.is_some() || limit.is_some_and(|cap| stats.read >= cap) {
        return Vec::new();
    }
    let mut bytes = vec![0; limit.map_or(16384, |cap| (cap - stats.read).min(16384) as usize)];
    match pipe.read(&mut bytes) {
        Ok(0) => {
            stats.eof = true;
            bytes.clear();
        }
        Ok(count) => {
            bytes.truncate(count);
            stats.read = stats.read.saturating_add(count as u64);
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
        progress.overflowed = true;
    }
    if progress.discard {
        progress.stdout.discarded = progress.stdout.discarded.saturating_add(out);
        progress.stderr.discarded = progress.stderr.discarded.saturating_add(err);
    } else {
        progress.stdout.enqueued = progress.stdout.enqueued.saturating_add(out);
        progress.stderr.enqueued = progress.stderr.enqueued.saturating_add(err);
    }
}
fn run(
    origin: CaptureOrigin,
    mut out: File,
    mut err: File,
    deadline: Option<Instant>,
    sender: SyncSender<CaptureBatch>,
    shared: Arc<Shared>,
) -> ReaderFinal {
    let mut progress = ReaderProgress::default();
    let mut rounds: u64 = 0;
    let limit = deadline.map(|_| STREAM_LIMIT);
    while (deadline.is_none() || rounds < 4096)
        && deadline.is_none_or(|end| Instant::now() < end)
        && !shared.stop.load(Ordering::Acquire)
    {
        if finished(&progress.stdout) && finished(&progress.stderr) {
            break;
        }
        if limit.is_some_and(|cap| progress.stdout.read >= cap || progress.stderr.read >= cap) {
            break;
        }
        rounds = rounds.saturating_add(1);
        if let Err(error) = poll_pair(
            &out,
            &err,
            &progress,
            deadline.unwrap_or_else(|| Instant::now() + Duration::from_millis(20)),
        ) {
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
            stdout: read_one(&mut out, &mut progress.stdout, limit),
            stderr: read_one(&mut err, &mut progress.stderr, limit),
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
