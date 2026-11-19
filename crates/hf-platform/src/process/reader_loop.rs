use super::reader::{
    CaptureOrigin, PrivateFixtureCaptureReader, ReaderFinal, ReaderProgress, ReaderStreamStats,
    Shared,
};
use super::reader_queue::{BATCH_CAP, Queue, ReaderPolicy, WAIT_STEP};
use nix::poll::{PollFd, PollFlags, poll};
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::AsFd;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::sync_channel,
};
use std::time::{Duration, Instant};
const STREAM_LIMIT: u64 = 8 * 1024 * 1024;
/// The private fixed-helper reader: deadline-bound, with the fixture policy.
pub(super) fn start(
    origin: CaptureOrigin,
    out: File,
    err: File,
    deadline: Instant,
) -> io::Result<PrivateFixtureCaptureReader> {
    start_policy(
        origin,
        (out, err),
        Some(deadline.min(Instant::now() + Duration::from_secs(8))),
        ReaderPolicy::fixture(),
    )
}
/// The production reader: no deadline, the caller's policy.
pub(super) fn start_production(
    origin: CaptureOrigin,
    out: File,
    err: File,
    policy: ReaderPolicy,
) -> io::Result<PrivateFixtureCaptureReader> {
    start_policy(origin, (out, err), None, policy)
}
fn start_policy(
    origin: CaptureOrigin,
    (out, err): (File, File),
    deadline: Option<Instant>,
    policy: ReaderPolicy,
) -> io::Result<PrivateFixtureCaptureReader> {
    let shared = Arc::new(Shared {
        progress: Mutex::new(ReaderProgress::default()),
        final_value: OnceLock::new(),
        stop: AtomicBool::new(false),
        discard: AtomicBool::new(false),
        dequeued: AtomicU64::new(0),
    });
    let (sender, receiver) = sync_channel(4);
    let work = shared.clone();
    let queue = Queue::new(origin.clone(), policy, sender);
    let worker = std::thread::Builder::new()
        .name("fixture-pipe-reader".into())
        .spawn(move || run(Pipes::new(out, err, deadline), queue, work))?;
    Ok(PrivateFixtureCaptureReader {
        origin,
        shared,
        receiver,
        worker: Some(worker),
        join_failed: false,
    })
}
struct Pipes {
    out: File,
    err: File,
    deadline: Option<Instant>,
    limit: Option<u64>,
    /// One reused buffer: a read lands here and is then moved into the pending batch.
    scratch: Vec<u8>,
}
impl Pipes {
    fn new(out: File, err: File, deadline: Option<Instant>) -> Self {
        Self {
            out,
            err,
            deadline,
            limit: deadline.map(|_| STREAM_LIMIT),
            scratch: vec![0; BATCH_CAP],
        }
    }
}
fn read_one(
    pipe: &mut File,
    stats: &mut ReaderStreamStats,
    limit: Option<u64>,
    room: usize,
    scratch: &mut [u8],
) -> usize {
    if stats.eof || stats.error.is_some() || room == 0 || limit.is_some_and(|cap| stats.read >= cap)
    {
        return 0;
    }
    let want = limit.map_or(room, |cap| room.min((cap - stats.read) as usize));
    match pipe.read(&mut scratch[..want.min(BATCH_CAP)]) {
        Ok(0) => {
            stats.eof = true;
            0
        }
        Ok(count) => {
            stats.read = stats.read.saturating_add(count as u64);
            count
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            0
        }
        Err(error) => {
            stats.error = Some(error.to_string());
            0
        }
    }
}
/// Reads each stream that has room into the pending batch.
fn round(pipes: &mut Pipes, progress: &mut ReaderProgress, queue: &mut Queue) {
    for err in [false, true] {
        let room = queue.room(err);
        let (pipe, stats) = if err {
            (&mut pipes.err, &mut progress.stderr)
        } else {
            (&mut pipes.out, &mut progress.stdout)
        };
        let count = read_one(pipe, stats, pipes.limit, room, &mut pipes.scratch);
        queue.absorb(progress, &pipes.scratch[..count], err);
    }
}
fn run(mut pipes: Pipes, mut queue: Queue, shared: Arc<Shared>) -> ReaderFinal {
    let mut progress = ReaderProgress::default();
    let mut rounds: u64 = 0;
    let deadline = pipes.deadline;
    while (deadline.is_none() || rounds < 4096)
        && deadline.is_none_or(|end| Instant::now() < end)
        && !shared.stop.load(Ordering::Acquire)
    {
        if finished(&progress.stdout) && finished(&progress.stderr) && queue.is_empty() {
            break;
        }
        if pipes
            .limit
            .is_some_and(|cap| progress.stdout.read >= cap || progress.stderr.read >= cap)
        {
            break;
        }
        rounds = rounds.saturating_add(1);
        if let Err(error) = poll_pair(&pipes, &progress, &queue) {
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
        round(&mut pipes, &mut progress, &mut queue);
        queue.flush(&mut progress, &shared);
        shared.publish(&progress);
    }
    let stopped = shared.stop.load(Ordering::Acquire);
    queue.drop_pending(&mut progress);
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
/// Waits for readable data on a stream that has room. While a batch is pending the wait
/// is one short step, so a dequeue, a discard or a stop is noticed promptly; a stream at
/// its cap is left out of the poll set and its pipe backs up to the child.
fn poll_pair(pipes: &Pipes, progress: &ReaderProgress, queue: &Queue) -> io::Result<()> {
    let mut descriptors = Vec::with_capacity(2);
    if !finished(&progress.stdout) && queue.room(false) > 0 {
        descriptors.push(PollFd::new(pipes.out.as_fd(), PollFlags::POLLIN));
    }
    if !finished(&progress.stderr) && queue.room(true) > 0 {
        descriptors.push(PollFd::new(pipes.err.as_fd(), PollFlags::POLLIN));
    }
    let limit = if queue.is_empty() {
        Duration::from_millis(20)
    } else {
        WAIT_STEP
    };
    let millis = pipes
        .deadline
        .unwrap_or_else(|| Instant::now() + limit)
        .saturating_duration_since(Instant::now())
        .min(limit)
        .as_millis() as u16;
    match poll(&mut descriptors, millis) {
        Ok(_) | Err(nix::errno::Errno::EINTR) => Ok(()),
        Err(error) => Err(error.into()),
    }
}
