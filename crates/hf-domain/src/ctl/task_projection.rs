//! The single classifier that projects a recorded task and the verdict on its process into
//! the status, activity and reconciliation fields every read reports.
use super::caller::{IdentityVerdict, TERMINAL_STATUSES};

/// A recorded outcome might be a lie about a process that is still running.
pub fn is_unobserved(status: &str, exit_code: Option<i64>) -> bool {
    status == "running" || exit_code.is_none()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Projection {
    pub status: String,
    pub active: bool,
    pub identity_state: &'static str,
    pub needs_reconciliation: bool,
    /// Set when a dead unobserved task is reported `unknown`.
    pub note: Option<&'static str>,
    /// No owner facts exist yet, so every task is observed by an unknown mode.
    pub observation_mode: &'static str,
    pub recovered: bool,
}

/// `verdict` is read only for an unobserved record; a record with no process row is
/// `Undecidable`. Live verdicts decide, never a recorded value.
const ENDED_NOTE: &str = "process ended; outcome needs inspection";

pub fn project(status: &str, exit_code: Option<i64>, verdict: IdentityVerdict) -> Projection {
    let unobserved = is_unobserved(status, exit_code);
    let state = if !unobserved {
        "dead"
    } else {
        match verdict {
            IdentityVerdict::Alive => "alive",
            IdentityVerdict::Dead => "dead",
            IdentityVerdict::Undecidable => "uncertain",
        }
    };
    let active = unobserved && state != "dead";
    let known = ["cancelled", "completed", "timed_out"].contains(&status);
    let (shown, needs, note) = if active {
        let needs = state == "uncertain" || TERMINAL_STATUSES.contains(&status);
        ("running", needs, None)
    } else if !unobserved || known {
        (status, false, None)
    } else {
        ("unknown", true, Some(ENDED_NOTE))
    };
    Projection {
        status: shown.to_string(),
        active,
        identity_state: state,
        needs_reconciliation: needs,
        note,
        observation_mode: "unknown",
        recovered: true,
    }
}
