use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

static FIXTURE_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Serializes only Store fixture setup/use across host unit tests. Each case's
/// actual reader/owner concurrency remains governed by its explicit gates.
pub(super) fn case_guard() -> MutexGuard<'static, ()> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        match FIXTURE_TEST_LOCK.try_lock() {
            Ok(guard) => return guard,
            Err(TryLockError::Poisoned(error)) => return error.into_inner(),
            Err(TryLockError::WouldBlock) if Instant::now() < until => {
                std::thread::yield_now();
            }
            Err(TryLockError::WouldBlock) => panic!("host fixture test admission deadline"),
        }
    }
}
