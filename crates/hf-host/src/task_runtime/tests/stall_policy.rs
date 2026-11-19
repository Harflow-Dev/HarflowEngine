//! The reader's stall bound against the deadlines it must stay below.
use crate::task_runtime::io;
use hf_platform::process::STALL_BOUND;
use std::time::Duration;

#[test]
fn stall_bound_is_two_seconds_and_below_io_deadline() {
    assert_eq!(STALL_BOUND, Duration::from_secs(2));
    // The exchange and input deadlines are `io::deadline()`; the stall must expire well
    // inside them so a stalled capture resolves as known loss, not as a poisoned owner.
    let slack = io::deadline().saturating_duration_since(std::time::Instant::now());
    assert!(STALL_BOUND < slack.min(Duration::from_secs(5)));
    assert!(STALL_BOUND * 2 < Duration::from_secs(5));
}
