//! The capture reader's pending batch, stall rule and loss accounting.
//!
//! The reader reads into one pending batch (at most `BATCH_CAP` bytes per stream) and
//! hands it to the bounded FIFO with `try_send`. A full FIFO keeps the batch pending and
//! the reader keeps reading the streams that still have room; a stream at its cap leaves
//! the poll set, so its pipe fills and the child blocks. Only a stall (the FIFO stayed
//! full with a pending batch and nothing was dequeued for the stall bound) or a
//! disconnect makes the discard sticky. A requested discard or a stop counts the pending
//! bytes as discarded without a loss cause.
use super::reader::{
    CaptureBatch, CaptureOrigin, LossCause, ReaderProgress, ReaderStreamStats, Shared,
};
use std::sync::{
    atomic::Ordering,
    mpsc::{SyncSender, TrySendError},
};
use std::time::{Duration, Instant};

/// The most bytes of one stream in one batch (the raw append limit).
pub const BATCH_CAP: usize = 16384;
/// How long a full queue with a pending batch may see no dequeue before the loss is
/// recorded. An accepted policy bound, not proof that the consumer is unhealthy: it is
/// above the measured healthy gaps (drain 11-16 ms, commit p99 14 ms, maintenance 100 ms,
/// worst observed 0.5 s) and below the 5 s exchange and input deadlines.
pub const STALL_BOUND: Duration = Duration::from_secs(2);
/// The sleep granularity while a send is waiting for room.
pub(super) const WAIT_STEP: Duration = Duration::from_millis(2);

/// How the reader treats a full queue. The fields are private; the constructors are the
/// only way to build a policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReaderPolicy {
    coalesce: bool,
    stall: Duration,
}
impl ReaderPolicy {
    /// Coalescing with the production stall bound.
    pub fn production() -> Self {
        Self {
            coalesce: true,
            stall: STALL_BOUND,
        }
    }
    /// The private fixed-helper behaviour: no coalescing, the first full queue is a loss.
    pub fn fixture() -> Self {
        Self {
            coalesce: false,
            stall: Duration::ZERO,
        }
    }
    /// Coalescing with an explicit stall bound.
    pub fn with_stall(stall: Duration) -> Self {
        Self {
            coalesce: true,
            stall,
        }
    }
    fn bound(self) -> Duration {
        if self.coalesce {
            self.stall
        } else {
            Duration::ZERO
        }
    }
}
/// Whether a queue that has been full since `blocked_since` has stalled at `now`.
pub(super) fn stalled(blocked_since: Option<Instant>, now: Instant, bound: Duration) -> bool {
    blocked_since.is_some_and(|since| now.saturating_duration_since(since) >= bound)
}
fn stream(progress: &mut ReaderProgress, err: bool) -> &mut ReaderStreamStats {
    if err {
        &mut progress.stderr
    } else {
        &mut progress.stdout
    }
}
pub(super) struct Queue {
    origin: CaptureOrigin,
    policy: ReaderPolicy,
    sender: SyncSender<CaptureBatch>,
    out: Vec<u8>,
    err: Vec<u8>,
    blocked_since: Option<Instant>,
    seen: u64,
}
impl Queue {
    pub fn new(
        origin: CaptureOrigin,
        policy: ReaderPolicy,
        sender: SyncSender<CaptureBatch>,
    ) -> Self {
        Self {
            origin,
            policy,
            sender,
            out: Vec::new(),
            err: Vec::new(),
            blocked_since: None,
            seen: 0,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.out.is_empty() && self.err.is_empty()
    }
    /// How many more bytes the pending batch takes from a stream.
    pub fn room(&self, err: bool) -> usize {
        BATCH_CAP - if err { self.err.len() } else { self.out.len() }
    }
    /// Accounts bytes just read from one stream: pending, or discarded once the discard is
    /// sticky. `stats.read` is already updated by the caller.
    pub fn absorb(&mut self, progress: &mut ReaderProgress, bytes: &[u8], err: bool) {
        if bytes.is_empty() {
            return;
        }
        if progress.discard {
            let stats = stream(progress, err);
            stats.discarded = stats.discarded.saturating_add(bytes.len() as u64);
            return;
        }
        let held = if err { &mut self.err } else { &mut self.out };
        if held.capacity() == 0 {
            held.reserve_exact(BATCH_CAP);
        }
        held.extend_from_slice(bytes);
        stream(progress, err).pending = held.len() as u64;
    }
    /// Counts every pending byte as discarded.
    pub fn drop_pending(&mut self, progress: &mut ReaderProgress) {
        for (held, err) in [(&mut self.out, false), (&mut self.err, true)] {
            let stats = stream(progress, err);
            stats.discarded = stats.discarded.saturating_add(held.len() as u64);
            stats.pending = 0;
            held.clear();
        }
        self.blocked_since = None;
    }
    /// One send attempt and the stall rule. A requested discard drops the pending bytes.
    pub fn flush(&mut self, progress: &mut ReaderProgress, shared: &Shared) {
        progress.discard |= shared.discard.load(Ordering::Acquire);
        if progress.discard {
            self.drop_pending(progress);
            return;
        }
        if self.is_empty() {
            return;
        }
        let dequeued = shared.dequeued.load(Ordering::Acquire);
        if dequeued != self.seen {
            self.seen = dequeued;
            self.blocked_since = self.blocked_since.map(|_| Instant::now());
        }
        let batch = CaptureBatch {
            origin: self.origin.clone(),
            stdout: std::mem::take(&mut self.out),
            stderr: std::mem::take(&mut self.err),
        };
        let lengths = (batch.stdout.len() as u64, batch.stderr.len() as u64);
        match self.sender.try_send(batch) {
            Ok(()) => {
                self.delivered(progress, lengths);
            }
            Err(TrySendError::Full(batch)) => {
                self.restore(batch);
                let now = Instant::now();
                let since = *self.blocked_since.get_or_insert(now);
                if stalled(Some(since), now, self.policy.bound()) {
                    self.overflow(progress, LossCause::Stall);
                }
            }
            Err(TrySendError::Disconnected(batch)) => {
                self.restore(batch);
                self.overflow(progress, LossCause::Disconnect);
            }
        }
    }
    fn delivered(&mut self, progress: &mut ReaderProgress, (out, err): (u64, u64)) {
        for (count, is_err) in [(out, false), (err, true)] {
            let stats = stream(progress, is_err);
            stats.enqueued = stats.enqueued.saturating_add(count);
            stats.pending = 0;
        }
        self.blocked_since = None;
    }
    fn restore(&mut self, batch: CaptureBatch) {
        self.out = batch.stdout;
        self.err = batch.stderr;
    }
    fn overflow(&mut self, progress: &mut ReaderProgress, cause: LossCause) {
        progress.discard = true;
        progress.overflowed = true;
        progress.overflow_cause.get_or_insert(cause);
        self.drop_pending(progress);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hf_domain::{
        ownership::{Fence, Id, OwnerClaim},
        process::ProcessIdentity,
    };
    use std::sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64},
        mpsc::{Receiver, sync_channel},
    };

    fn id(digit: char) -> Id {
        Id::new(digit.to_string().repeat(32)).unwrap()
    }
    fn process(pid: u32) -> ProcessIdentity {
        ProcessIdentity::new(
            pid,
            1_768_000_010,
            u64::from(pid),
            "01234567-89ab-cdef-0123-456789abcdef".into(),
        )
        .unwrap()
    }
    fn origin() -> CaptureOrigin {
        CaptureOrigin {
            child: process(7200),
            fence: Fence {
                operation: id('1'),
                generation: id('2'),
                attempt: id('3'),
                owner: OwnerClaim {
                    owner: id('1'),
                    nonce: id('4'),
                    process: process(7100),
                },
                revision: 2,
            },
        }
    }
    fn shared() -> Shared {
        Shared {
            progress: Mutex::new(ReaderProgress::default()),
            final_value: OnceLock::new(),
            stop: AtomicBool::new(false),
            discard: AtomicBool::new(false),
            dequeued: AtomicU64::new(0),
        }
    }
    fn queue(policy: ReaderPolicy) -> (Queue, Receiver<CaptureBatch>) {
        let (sender, receiver) = sync_channel(4);
        (Queue::new(origin(), policy, sender), receiver)
    }
    fn read(queue: &mut Queue, progress: &mut ReaderProgress, bytes: &[u8]) {
        progress.stdout.read += bytes.len() as u64;
        queue.absorb(progress, bytes, false);
    }

    #[test]
    fn stalled_boundary() {
        let start = Instant::now();
        let bound = Duration::from_secs(2);
        assert!(!stalled(None, start + bound, bound));
        assert!(!stalled(
            Some(start),
            start + bound - Duration::from_nanos(1),
            bound
        ));
        assert!(stalled(Some(start), start + bound, bound));
        assert!(stalled(Some(start), start, Duration::ZERO));
    }
    #[test]
    fn disconnect_records_disconnect_cause_and_discards_pending() {
        let (mut queue, receiver) = queue(ReaderPolicy::production());
        let (mut progress, shared) = (ReaderProgress::default(), shared());
        read(&mut queue, &mut progress, b"held");
        assert_eq!(progress.stdout.pending, 4);
        drop(receiver);
        queue.flush(&mut progress, &shared);
        assert!(progress.overflowed && progress.discard);
        assert_eq!(progress.overflow_cause, Some(LossCause::Disconnect));
        assert_eq!((progress.stdout.pending, progress.stdout.discarded), (0, 4));
        read(&mut queue, &mut progress, b"more");
        assert_eq!((progress.stdout.pending, progress.stdout.discarded), (0, 8));
        assert_eq!(
            progress.stdout.read,
            progress.stdout.enqueued + progress.stdout.discarded
        );
    }
    #[test]
    fn a_dequeue_resets_the_stall_timer_and_the_pending_batch_is_retried() {
        let (mut queue, receiver) = queue(ReaderPolicy::with_stall(Duration::from_millis(80)));
        let (mut progress, shared) = (ReaderProgress::default(), shared());
        for i in 0..4 {
            read(&mut queue, &mut progress, &[i]);
            queue.flush(&mut progress, &shared);
        }
        read(&mut queue, &mut progress, b"p");
        queue.flush(&mut progress, &shared);
        assert_eq!(
            progress.stdout.pending, 1,
            "the queue is full: the batch stays pending"
        );
        // Dequeue often enough that the bound never elapses without progress.
        for _ in 0..6 {
            std::thread::sleep(Duration::from_millis(50));
            receiver.try_recv().ok();
            shared.dequeued.fetch_add(1, Ordering::AcqRel);
            queue.flush(&mut progress, &shared);
            read(&mut queue, &mut progress, b"q");
            queue.flush(&mut progress, &shared);
        }
        assert!(!progress.overflowed, "{progress:?}");
        // No dequeue for longer than the bound is a stall.
        std::thread::sleep(Duration::from_millis(150));
        queue.flush(&mut progress, &shared);
        std::thread::sleep(Duration::from_millis(150));
        queue.flush(&mut progress, &shared);
        assert_eq!(progress.overflow_cause, Some(LossCause::Stall));
        assert_eq!(progress.stdout.pending, 0);
    }
    #[test]
    fn fixture_policy_loses_on_the_first_full_queue() {
        let (mut queue, _receiver) = queue(ReaderPolicy::fixture());
        let (mut progress, shared) = (ReaderProgress::default(), shared());
        for i in 0..5 {
            read(&mut queue, &mut progress, &[i]);
            queue.flush(&mut progress, &shared);
        }
        assert_eq!(progress.overflow_cause, Some(LossCause::Stall));
        assert_eq!(
            (progress.stdout.enqueued, progress.stdout.discarded),
            (4, 1)
        );
    }
    #[test]
    fn requested_discard_counts_pending_without_a_cause() {
        let (mut queue, _receiver) = queue(ReaderPolicy::production());
        let (mut progress, shared) = (ReaderProgress::default(), shared());
        read(&mut queue, &mut progress, b"abc");
        shared.discard.store(true, Ordering::Release);
        queue.flush(&mut progress, &shared);
        assert!(progress.discard && !progress.overflowed);
        assert_eq!(progress.overflow_cause, None);
        assert_eq!((progress.stdout.pending, progress.stdout.discarded), (0, 3));
    }
    #[test]
    fn batch_cap_matches_the_raw_append_limit() {
        assert_eq!(BATCH_CAP, crate::task_runtime::raw::APPEND_CAP);
    }
}
