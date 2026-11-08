//! The recovery-activity page: display only, and empty unless capture loss is confirmed.
use super::caller::TERMINAL_STATUSES;
use crate::event_read::CaptureState;

pub const NOT_CONFIRMED: &str = "Transcript recovery requires confirmed capture loss";
pub const UNSUPPORTED: &str = "Backend transcript recovery is unsupported";
pub const NOT_AVAILABLE: &str = "Transcript recovery is not available in this engine";

/// The backends the reference registers, in sorted order.
pub const BACKENDS: [&str; 6] = [
    "antigravity",
    "claude",
    "codex",
    "cursor",
    "opencode",
    "vibe",
];

/// The backend that has a transcript to recover from.
const TRANSCRIPT_BACKEND: &str = "claude";

/// The notice of the empty page a task's capture and backend allow.
pub fn notice(capture: CaptureState, backend: &str) -> &'static str {
    if capture != CaptureState::LostWithOwner {
        NOT_CONFIRMED
    } else if backend == TRANSCRIPT_BACKEND {
        NOT_AVAILABLE
    } else {
        UNSUPPORTED
    }
}

/// The capture of a task: observed terminal when its record says it finished with an exit
/// code; otherwise only a stored live or lost capture passes through from the stream row.
pub fn capture(status: &str, exit_code: Option<i64>, stored: Option<CaptureState>) -> CaptureState {
    if exit_code.is_some() && TERMINAL_STATUSES.contains(&status) {
        return CaptureState::ObservedTerminal;
    }
    match stored {
        Some(CaptureState::Live) => CaptureState::Live,
        Some(CaptureState::LostWithOwner) => CaptureState::LostWithOwner,
        _ => CaptureState::Unknown,
    }
}
