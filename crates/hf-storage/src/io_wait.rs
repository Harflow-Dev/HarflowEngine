//! The pre-effect wait for `io.lock` of a commit under `IoWait::PollUntilDeadline`.
//!
//! `io.lock` acquisition never blocks: a held lock is `Busy`. The task writer would
//! otherwise fail a command (and be discarded) because a sibling holds the lock for
//! maintenance. Such a commit retries only the acquisition, before any transaction starts,
//! every `IO_POLL` until `IO_RESERVE` before its deadline. Nothing after the acquisition is
//! retried: not the connection-mutex `Busy` of admission, not an identity, sidecar or fence
//! error, not SQLite `BUSY` inside the transaction.
//!
//! The holder of the handle is single-threaded, so holding the connection mutex while
//! polling only makes other users of the same `Store` see `Busy`; a same-thread
//! re-acquisition of `io.lock` would spin to the budget instead of failing at once, and no
//! such path exists.
//!
//! `IO_RESERVE` is a heuristic allowance for the immediate transaction, the action, the
//! closing checks, the watermark flush and the acknowledgement's transit; it is not an
//! established completion bound.
use crate::{Error, Result, Store, wal_owner::IoWait};
use hf_application::lease::LeaseGuard;
use std::time::{Duration, Instant};

/// Pause between attempts to take `io.lock`.
pub const IO_POLL: Duration = Duration::from_millis(5);
/// Time before its deadline at which a commit stops retrying `io.lock`.
pub const IO_RESERVE: Duration = Duration::from_secs(1);

/// Calls `acquire` until it succeeds or fails with anything but `Busy`. After a `Busy` it
/// retries only while `now + poll` is before `cutoff`, checked before each sleep and again
/// after it (an oversleep past `cutoff` returns the last error without another attempt).
/// With `recheck` false the sleep is not followed by that check, so one more attempt follows
/// each sleep (the semantics of the verifier's record lock). The first attempt always
/// happens. The flag says at least one `Busy` was observed before
/// the acquisition succeeded.
pub(crate) fn poll_until<T>(
    mut acquire: impl FnMut() -> Result<T>,
    now: impl Fn() -> Instant,
    sleep: impl Fn(Duration),
    poll: Duration,
    cutoff: Instant,
    recheck: bool,
) -> Result<(T, bool)> {
    let mut waited = false;
    loop {
        match acquire() {
            Ok(value) => return Ok((value, waited)),
            Err(Error::Busy) => {
                waited = true;
                if now() + poll >= cutoff {
                    return Err(Error::Busy);
                }
                sleep(poll);
                if recheck && now() >= cutoff {
                    return Err(Error::Busy);
                }
            }
            Err(error) => return Err(error),
        }
    }
}

impl Store {
    /// `io.lock` for a commit under this handle's `IoWait`, with whether it waited. The
    /// operation `deadline` goes to every attempt; the retry cutoff is `deadline` minus
    /// `IO_RESERVE`, and without one (an unrepresentable instant) there is one attempt.
    pub(crate) fn acquire_io(&self, deadline: Instant) -> Result<(Box<dyn LeaseGuard>, bool)> {
        if self.options.io_wait == IoWait::PollUntilDeadline
            && let Some(cutoff) = deadline.checked_sub(IO_RESERVE)
        {
            return self.binding.io_polling(deadline, IO_POLL, Some(cutoff));
        }
        Ok((self.binding.io(deadline)?, false))
    }
}
