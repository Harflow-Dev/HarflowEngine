#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The retry function of the pre-effect `io.lock` wait, with an injected clock and sleep.
use crate::{Error, Result, io_wait::poll_until};
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(5);

/// Runs `poll_until` with a clock that only `sleep` advances, by `step` per call (a step
/// longer than the poll is an oversleep).
fn run(
    outcomes: &[Result<u32>],
    cutoff_after: Duration,
    step: Duration,
    recheck: bool,
) -> (Result<(u32, bool)>, usize) {
    let start = Instant::now();
    let clock = Cell::new(start);
    let attempts = Cell::new(0);
    let result = poll_until(
        || {
            let index = attempts.get();
            attempts.set(index + 1);
            match outcomes.get(index).or(outcomes.last()).unwrap() {
                Ok(value) => Ok(*value),
                Err(Error::Busy) => Err(Error::Busy),
                Err(Error::Fenced) => Err(Error::Fenced),
                Err(_) => Err(Error::Replaced),
            }
        },
        || clock.get(),
        |_| clock.set(clock.get() + step),
        POLL,
        start + cutoff_after,
        recheck,
    );
    (result, attempts.get())
}

#[test]
fn oversleep_past_the_cutoff_makes_no_further_attempt() {
    // One sleep jumps 100 ms past a 20 ms cutoff: the last error is returned, no new attempt.
    let (result, attempts) = run(
        &[Err(Error::Busy)],
        Duration::from_millis(20),
        Duration::from_millis(100),
        true,
    );
    assert!(matches!(result, Err(Error::Busy)));
    assert_eq!(attempts, 1);
}

#[test]
fn a_past_cutoff_still_makes_the_first_attempt_and_no_retry() {
    let (result, attempts) = run(&[Err(Error::Busy)], Duration::ZERO, POLL, true);
    assert!(matches!(result, Err(Error::Busy)));
    assert_eq!(attempts, 1);
    let (result, attempts) = run(&[Ok(9)], Duration::ZERO, POLL, true);
    assert_eq!(result.unwrap(), (9, false));
    assert_eq!(attempts, 1);
}

#[test]
fn a_busy_lock_is_retried_inside_the_cutoff_and_reports_the_wait() {
    let (result, attempts) = run(
        &[Err(Error::Busy), Err(Error::Busy), Ok(3)],
        Duration::from_millis(100),
        POLL,
        true,
    );
    assert_eq!(result.unwrap(), (3, true));
    assert_eq!(attempts, 3);
}

#[test]
fn other_errors_are_returned_at_once_and_the_retry_stops_at_the_cutoff() {
    let (result, attempts) = run(
        &[Err(Error::Fenced)],
        Duration::from_millis(100),
        POLL,
        true,
    );
    assert!(matches!(result, Err(Error::Fenced)));
    assert_eq!(attempts, 1);
    // 5 ms polls, 22 ms cutoff: attempts at 0, 5, 10, 15 and 20; at 20 the next poll would pass 22.
    let (result, attempts) = run(&[Err(Error::Busy)], Duration::from_millis(22), POLL, true);
    assert!(matches!(result, Err(Error::Busy)));
    assert_eq!(attempts, 5);
}

#[test]
fn the_verifier_mode_makes_one_more_attempt_after_a_sleep() {
    // Without the post-sleep check (the verifier's original loop) an oversleep still retries.
    let (result, attempts) = run(
        &[Err(Error::Busy), Ok(4)],
        Duration::from_millis(20),
        Duration::from_millis(100),
        false,
    );
    assert_eq!(result.unwrap(), (4, true));
    assert_eq!(attempts, 2);
    let (result, attempts) = run(
        &[Err(Error::Busy)],
        Duration::from_millis(20),
        Duration::from_millis(100),
        true,
    );
    assert!(matches!(result, Err(Error::Busy)));
    assert_eq!(attempts, 1);
}
