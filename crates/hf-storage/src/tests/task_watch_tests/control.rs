#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::support::*;
use crate::Limits;
use hf_application::task_watch::{TaskWatchPort, WatchError};
use std::{
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};
#[test]
fn watch_cancellation_and_expired_caller_deadline_never_look_like_timeout() {
    let fixture = Fixture::new("cancel-deadline");
    let _store = fixture.initialize();
    let baseline = watch(&fixture.root(), Clock::default())
        .execute(&request(None, 0), deadline())
        .unwrap()
        .cursor
        .unwrap();
    let cancelled = Clock::default();
    cancelled.cancel();
    assert_eq!(
        watch(&fixture.root(), cancelled).execute(&request(Some(baseline.clone()), 3), deadline()),
        Err(WatchError::Cancelled)
    );
    assert_eq!(
        watch(&fixture.root(), Clock::default()).execute(
            &request(Some(baseline), 3),
            Instant::now() - Duration::from_millis(1)
        ),
        Err(WatchError::DeadlineExceeded)
    );
}

#[test]
fn cancellation_and_deadline_during_final_cursor_encoding_override_a_success_result() {
    for cancelled in [true, false] {
        let fixture = Fixture::new("final-encode-check");
        let _store = fixture.initialize();
        let control = Clock::default();
        let callback_clock = control.clone();
        let callback: Action = Arc::new(move || {
            if cancelled {
                callback_clock.cancel();
            } else {
                callback_clock.set_now(Instant::now() + Duration::from_secs(10));
            }
        });
        let session = crate::WatchSession::new(
            fixture.root(),
            Limits::default(),
            EncodeActionCodec(callback),
            control,
            crate::tests::lease::lease_port(),
        );
        let result = session.execute(&request(None, 0), deadline());
        assert_eq!(
            result,
            Err(if cancelled {
                WatchError::Cancelled
            } else {
                WatchError::DeadlineExceeded
            })
        );
    }
}

#[derive(Clone)]
struct EnteredRealControl {
    control: crate::RealWatchControl,
    entered: Arc<Mutex<Option<mpsc::Sender<()>>>>,
}
impl hf_application::task_watch::WatchControl for EnteredRealControl {
    fn now(&self) -> Instant {
        self.control.now()
    }
    fn check(&self) -> Result<(), WatchError> {
        self.control.check()
    }
    fn wait_until(&self, target: Instant) -> Result<(), WatchError> {
        if let Some(send) = self.entered.lock().unwrap().take() {
            let _ = send.send(());
        }
        self.control.wait_until(target)
    }
}

#[test]
fn cloned_real_control_cancels_an_actual_condvar_wait_without_sleep_ordering() {
    let fixture = Fixture::new("real-cancel");
    let _store = fixture.initialize();
    let baseline = watch(&fixture.root(), Clock::default())
        .execute(&request(None, 0), deadline())
        .unwrap()
        .cursor
        .unwrap();
    let control = crate::RealWatchControl::default();
    let (send_entered, entered) = mpsc::channel();
    let wrapper = EnteredRealControl {
        control: control.clone(),
        entered: Arc::new(Mutex::new(Some(send_entered))),
    };
    let path = fixture.root();
    let (send_result, result) = mpsc::channel();
    let handle = thread::spawn(move || {
        let session = crate::WatchSession::new(
            path,
            Limits::default(),
            Codec,
            wrapper,
            crate::tests::lease::lease_port(),
        );
        let _ = send_result.send(session.execute(&request(Some(baseline), 4), deadline()));
    });
    entered
        .recv_timeout(Duration::from_secs(3))
        .expect("watch must enter its interruptible condvar wait");
    control.cancel().unwrap();
    assert_eq!(
        result
            .recv_timeout(Duration::from_secs(3))
            .expect("cancelled wait must finish boundedly"),
        Err(WatchError::Cancelled)
    );
    let stop_by = Instant::now() + Duration::from_secs(1);
    while !handle.is_finished() && Instant::now() < stop_by {
        thread::yield_now();
    }
    assert!(
        handle.is_finished(),
        "watch thread reported completion but did not exit"
    );
    handle.join().unwrap();
}
