//! Whole-store verification on a cadence, and the fence a failed check leaves.
//!
//! The due record `store.verify` holds `harflow-verify-1\n<generation>\n<due_unix_s>\n`
//! (at most 128 bytes). It is created lazily by the first passing check, never at
//! initialization. `verify_in_place` runs one bounded `PRAGMA quick_check(1)` on a
//! read-only connection under the handle's shared lease and without `io.lock`, so a
//! commit is never delayed by the scan; `io.lock` is taken only for the short record or
//! fence write that follows a verdict (a commit that collides with that write can see a
//! short `Busy`).
use crate::{
    Error, Result, Store, connection, fence,
    qualify::{self, WalStamp},
    read_session::open_read_only,
    scan, small_file, wal,
};
use rusqlite::{Connection, ErrorCode};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Interval between passing checks of one generation.
pub const VERIFY_PERIOD: Duration = Duration::from_secs(24 * 60 * 60);
/// Budget for taking `io.lock` to record a verdict, counted from the verdict itself so
/// that a failure found at the end of the scan deadline is still fenced. A busy lock is
/// retried every `RECORD_RETRY` until the budget runs out.
pub const RECORD_BUDGET: Duration = Duration::from_secs(2);
/// Pause between attempts to take `io.lock` for a record.
const RECORD_RETRY: Duration = Duration::from_millis(5);

const NAME: &str = "store.verify";
const HEADER: &str = "harflow-verify-1";
const BOUND: u64 = 128;

/// The outcome of one whole-store check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verification {
    /// The check passed; the next one is due one period later.
    Passed,
    /// The check found corruption; `store.fence` now refuses every write and read.
    Fenced,
    /// Interrupted at the deadline, busy, or an I/O failure: no fence, due time unchanged.
    Inconclusive,
}

impl Store {
    /// Whether a whole-store check is due at `now`. An absent, malformed or
    /// foreign-generation record is due, and so is a due time more than one period
    /// ahead of `now` (a clock that moved back). An oversized regular record is malformed,
    /// so due; a non-regular record (directory, link, FIFO) is `Incompatible`.
    pub fn verification_due(&self, now: SystemTime) -> Result<bool> {
        let bytes = match small_file::read_bounded(self.binding.root(), NAME, BOUND)? {
            small_file::Content::Bytes(bytes) => bytes,
            small_file::Content::Absent | small_file::Content::Oversized => return Ok(true),
        };
        let due =
            small_file::state(&bytes, HEADER, &self.identity.generation).and_then(parse_seconds);
        Ok(due.is_none_or(|due| is_due(due, unix_seconds(now))))
    }
    /// The cadence runner: one check when `verification_due(now)`, else `None`.
    pub fn verify_if_due(
        &self,
        now: SystemTime,
        deadline: Instant,
    ) -> Result<Option<Verification>> {
        if !self.verification_due(now)? {
            return Ok(None);
        }
        self.verify_at(now, deadline).map(Some)
    }
    /// One whole-store check now, whatever the due record says.
    pub fn verify_in_place(&self, deadline: Instant) -> Result<Verification> {
        self.verify_at(SystemTime::now(), deadline)
    }
    fn verify_at(&self, now: SystemTime, deadline: Instant) -> Result<Verification> {
        connection::check_deadline(deadline)?;
        // Marker, header, journal, `-wal`, acceptance record and fence, as at open.
        if qualify::precheck(&self.binding)? != self.identity {
            return Err(Error::Replaced);
        }
        let stamp = qualify::wal_stamp(&self.binding)?;
        let verdict = {
            let connection = open_read_only(&self.binding, deadline)?;
            if connection::identity(&connection, deadline).map_err(scan::corruption)?
                != self.identity
            {
                return Err(Error::Incompatible);
            }
            check(&connection, deadline)?
        };
        self.confirm_unchanged(stamp)?;
        match verdict {
            Verification::Passed => self.record_due(now)?,
            Verification::Fenced => self.write_fence()?,
            Verification::Inconclusive => {}
        }
        Ok(verdict)
    }
    /// The scan saw this store: binding, `-wal` and marker are the ones checked before it.
    fn confirm_unchanged(&self, stamp: WalStamp) -> Result<()> {
        self.binding.check()?;
        qualify::recheck_wal(&self.binding, stamp)?;
        if wal::marker(&self.binding)? != self.identity {
            return Err(Error::Replaced);
        }
        Ok(())
    }
    /// Unflushed: a lost record only causes a repeated check.
    fn record_due(&self, now: SystemTime) -> Result<()> {
        let due = unix_seconds(now).saturating_add(VERIFY_PERIOD.as_secs());
        let bytes = format!("{HEADER}\n{}\n{due}\n", self.identity.generation);
        let _io = self.record_lock()?;
        fence::check(self.binding.root(), &self.identity.generation)?;
        small_file::replace(
            self.binding.root(),
            NAME,
            bytes.as_bytes(),
            small_file::Durability::Unflushed,
        )
    }
    /// Any failure to record the fence after a corruption verdict (the lock budget, an
    /// I/O error writing, renaming or flushing it, a read-back mismatch) is
    /// `FenceUnrecorded` with its cause, so the caller always learns corruption was found.
    /// An existing fence must be absent or this generation's `failed`; any other fence
    /// (`Incompatible`) is returned as is and left in place, and a failure to read it
    /// (I/O, `ELOOP`) is `FenceUnrecorded` too.
    fn write_fence(&self) -> Result<()> {
        let unrecorded = |cause: Error| Error::FenceUnrecorded(Box::new(cause));
        let _io = self.record_lock().map_err(unrecorded)?;
        match fence::check(self.binding.root(), &self.identity.generation) {
            Ok(()) | Err(Error::Fenced) => {}
            // An existing fence that is not this generation's `failed` is left in place.
            Err(Error::Incompatible) => return Err(Error::Incompatible),
            Err(cause) => return Err(unrecorded(cause)),
        }
        fence::write(self.binding.root(), &self.identity.generation).map_err(unrecorded)
    }
    /// `io.lock` within `RECORD_BUDGET` from now. The lease port does not wait for a held
    /// lock, so `Busy` is retried after a short pause until the budget runs out.
    fn record_lock(&self) -> Result<Box<dyn hf_application::lease::LeaseGuard>> {
        let budget = Instant::now() + RECORD_BUDGET;
        loop {
            match self.binding.io(budget) {
                Err(Error::Busy) if Instant::now() + RECORD_RETRY < budget => {
                    std::thread::sleep(RECORD_RETRY);
                }
                other => return other,
            }
        }
    }
}

/// `quick_check` verdict. Corruption (`SQLITE_CORRUPT`, `SQLITE_NOTADB` or a verdict
/// other than `ok`, whenever it arrives) fences; an `ok` after the deadline, an interrupt,
/// busy or `SQLITE_IOERR` is inconclusive; any other error is returned unchanged and, like
/// an inconclusive check, leaves no record. (`Io` can only come from setting the busy
/// timeout before the scan.)
fn check(connection: &Connection, deadline: Instant) -> Result<Verification> {
    let outcome = scan::bounded_late(connection, deadline, |db| {
        db.query_row("PRAGMA quick_check(1)", [], |row| row.get::<_, String>(0))
    });
    match outcome {
        Ok(verdict) if verdict == "ok" => Ok(if Instant::now() < deadline {
            Verification::Passed
        } else {
            Verification::Inconclusive
        }),
        Ok(_) | Err(Error::Incompatible) => Ok(Verification::Fenced),
        Err(Error::DeadlineExceeded | Error::Busy | Error::Io(_)) => Ok(Verification::Inconclusive),
        Err(Error::Sqlite(error))
            if error.sqlite_error_code() == Some(ErrorCode::SystemIoFailure) =>
        {
            Ok(Verification::Inconclusive)
        }
        Err(error) => Err(error),
    }
}

fn is_due(due: u64, now: u64) -> bool {
    now >= due || due > now.saturating_add(VERIFY_PERIOD.as_secs())
}

/// Canonical decimal seconds: digits only, no sign and no leading zero.
fn parse_seconds(text: &str) -> Option<u64> {
    let canonical = !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    canonical.then(|| text.parse().ok()).flatten()
}

/// Seconds since the Unix epoch; a time before it counts as zero.
fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}
