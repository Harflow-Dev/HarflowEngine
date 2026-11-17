use crate::{Error, Result};
use hf_domain::task_lifecycle::*;
pub(crate) fn validate_observation(
    old: &ExecutionObservation,
    next: &ExecutionObservation,
) -> Result<()> {
    if next.normalized_event_bytes > 64 * 1024 * 1024 - 65_536
        || next.omitted_events > 0 && !next.observation_gap
        || next.obligations != Completeness::Unknown && next.obligation_witness.is_none()
        || next.obligation_witness.as_ref().is_some_and(|w| {
            w.source_start > w.source_end
                || w.source_end > next.stdout_end
                || w.observed_items.len() > 64
        })
        || old.obligation_witness.as_ref().is_some_and(|old_w| {
            old_w.observed_items.iter().any(|item| {
                item.failed
                    && next.obligation_witness.as_ref().is_none_or(|w| {
                        !w.observed_items.iter().any(|v| v.id == item.id && v.failed)
                    })
            })
        })
        || next.normalized_event_bytes != old.normalized_event_bytes
        || next.omitted_events < old.omitted_events
        || next.omitted_events > old.omitted_events && !next.observation_gap
        || next.obligations == Completeness::Complete
        || next.descendants.len() > 64
        || (!next.descendants.is_empty() && next.descendant_scan.is_none())
        || next.stdout_end > 64 * 1024 * 1024
        || next.stderr_end > 64 * 1024 * 1024
        || next.stdout_end.saturating_add(next.stderr_end) > 64 * 1024 * 1024
    {
        return Err(Error::LimitExceeded);
    }
    if next.stdout_end < old.stdout_end
        || next.stderr_end < old.stderr_end
        || old.observation_gap && !next.observation_gap
        || old.capture_known_loss && !next.capture_known_loss
        || old.event_projection_truncated && !next.event_projection_truncated
        || old.drained && !next.drained
        || old.input_closed && !next.input_closed
        || old.result.is_some() && old.result != next.result
        || old.exit != hf_domain::process::ProcessExit::Unobserved && old.exit != next.exit
        || old.requirement != next.requirement
    {
        return Err(Error::OwnershipConflict);
    }
    if next
        .result
        .as_ref()
        .is_some_and(|r| r.source_end > next.stdout_end)
    {
        return Err(Error::InvalidOwnership);
    }
    Ok(())
}
