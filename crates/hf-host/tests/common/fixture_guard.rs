use std::{
    sync::{Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
};

const ADMISSION_LIMIT: Duration = Duration::from_secs(45);
// B1 found a spawn-time file-lock inheritance window despite CLOEXEC. This
// serializes disposable fixture setup only; it does not qualify general launch.
static FIXTURE_BUSY: Mutex<bool> = Mutex::new(false);
static FIXTURE_AVAILABLE: Condvar = Condvar::new();

pub struct FixtureGuard {
    _private: (),
}

pub fn acquire() -> FixtureGuard {
    let until = Instant::now() + ADMISSION_LIMIT;
    let mut busy = lock_state();
    loop {
        let remaining = until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            drop(busy);
            panic!("timed out waiting for the serialized fixture admission slot");
        }
        if !*busy {
            *busy = true;
            drop(busy);
            return FixtureGuard { _private: () };
        }
        let (next, result) = FIXTURE_AVAILABLE
            .wait_timeout(busy, remaining)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        busy = next;
        if result.timed_out() && *busy {
            drop(busy);
            panic!("timed out waiting for the serialized fixture admission slot");
        }
    }
}

impl Drop for FixtureGuard {
    fn drop(&mut self) {
        let mut busy = lock_state();
        *busy = false;
        drop(busy);
        FIXTURE_AVAILABLE.notify_one();
    }
}

fn lock_state() -> MutexGuard<'static, bool> {
    FIXTURE_BUSY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
