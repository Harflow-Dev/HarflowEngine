use std::{
    sync::{Condvar, Mutex, OnceLock},
    time::{Duration, Instant},
};

fn state() -> &'static (Mutex<bool>, Condvar) {
    static STATE: OnceLock<(Mutex<bool>, Condvar)> = OnceLock::new();
    STATE.get_or_init(|| (Mutex::new(false), Condvar::new()))
}

pub(super) struct CoordinatorAdmission;
impl CoordinatorAdmission {
    pub(super) fn acquire() -> Self {
        let (lock, changed) = state();
        let end = Instant::now() + Duration::from_secs(5);
        let mut held = lock.lock().unwrap();
        while *held {
            let remaining = end.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "coordinator test admission timed out");
            let (next, timeout) = changed.wait_timeout(held, remaining).unwrap();
            held = next;
            assert!(
                !timeout.timed_out() || !*held,
                "coordinator test admission timed out"
            );
        }
        *held = true;
        Self
    }
}
impl Drop for CoordinatorAdmission {
    fn drop(&mut self) {
        let (lock, changed) = state();
        *lock.lock().unwrap() = false;
        changed.notify_one();
    }
}
