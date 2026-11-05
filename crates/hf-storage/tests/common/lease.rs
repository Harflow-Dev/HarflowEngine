#![allow(dead_code)]

use hf_application::lease::{LeaseError, LeaseGuard, LeaseMode, LeasePort};
use std::{
    fs::{File, OpenOptions},
    path::Path,
    sync::Arc,
    time::Instant,
};

/// Test-only adapter for storage behavior tests. Cross-process production lock
/// qualification belongs to hf-platform and uses its real fcntl coordinator.
#[derive(Default)]
pub struct FileLeasePort;

struct HeldFile(File);
impl LeaseGuard for HeldFile {}

impl LeasePort for FileLeasePort {
    fn acquire(
        &self,
        path: &Path,
        mode: LeaseMode,
        create: bool,
        deadline: Instant,
    ) -> Result<Box<dyn LeaseGuard>, LeaseError> {
        if Instant::now() >= deadline {
            return Err(LeaseError::DeadlineExceeded);
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        if create {
            options.create(true);
        }
        let file = options.open(path).map_err(|_| LeaseError::Invalid)?;
        let result = match mode {
            LeaseMode::Shared => file.try_lock_shared(),
            LeaseMode::Exclusive => file.try_lock(),
        };
        result.map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => LeaseError::Busy,
            std::fs::TryLockError::Error(_) => LeaseError::Unavailable,
        })?;
        if Instant::now() >= deadline {
            let _ = file.unlock();
            return Err(LeaseError::DeadlineExceeded);
        }
        Ok(Box::new(HeldFile(file)))
    }
}

pub fn lease_port() -> Arc<dyn LeasePort> {
    Arc::new(FileLeasePort)
}
