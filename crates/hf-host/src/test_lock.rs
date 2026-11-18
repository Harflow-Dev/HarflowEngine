#![allow(clippy::panic)]
//! The one lock that serializes host unit cases which open roots or start children.
//! It is compiled into both unit-test executables of this package (library and bin).
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

static FIXTURE_TEST_LOCK: Mutex<()> = Mutex::new(());

/// How long a case waits: cases that hold the lock include real child processes, so a
/// queued case waits for whole cases rather than for a unit step.
const ADMISSION: Duration = Duration::from_secs(180);

/// Blocks, tolerating a poisoned lock, until the shared lock is free.
pub(crate) fn serial_case() -> MutexGuard<'static, ()> {
    let until = Instant::now() + ADMISSION;
    loop {
        match FIXTURE_TEST_LOCK.try_lock() {
            Ok(guard) => return guard,
            Err(TryLockError::Poisoned(error)) => return error.into_inner(),
            Err(TryLockError::WouldBlock) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(TryLockError::WouldBlock) => panic!("host test admission deadline"),
        }
    }
}
/// Serializes only Store fixture setup/use across host unit tests. Each case's
/// actual reader/owner concurrency remains governed by its explicit gates.
#[allow(dead_code)]
pub(crate) fn case_guard() -> MutexGuard<'static, ()> {
    serial_case()
}
