//! Id resolution inside a snapshot, and the answers for a root with no store.
use super::{Candidate, ReadError, Resolution, StoreFailure, TaskSnapshotPort, UnknownReason};
use hf_domain::ctl::inspect::InspectReason;
use hf_domain::task_read::{Lookup, TaskId};

pub(crate) fn valid_id(raw: &str) -> Result<TaskId, ReadError> {
    TaskId::new(raw.to_string()).map_err(|_| ReadError::InvalidTaskId(raw.to_string()))
}

/// What a value that matches nothing is: unknown, with the reason a prefix lookup gives.
pub fn missing_resolution(id: &TaskId) -> Resolution {
    let reason = if id.canonical() {
        None
    } else if id.may_prefix() {
        Some(UnknownReason::NoPrefixMatch)
    } else {
        Some(UnknownReason::PrefixTooShort)
    };
    Resolution::Unknown {
        input: id.as_str().to_string(),
        reason,
    }
}

/// A task the lookup found in this snapshot has no record in it: the store contradicts
/// itself, which is a store failure, never an unknown task.
pub(crate) fn drift() -> ReadError {
    ReadError::Store(StoreFailure::Incompatible {
        reason: InspectReason::InfrastructureUnavailable,
        message: "Task history cannot be read".to_string(),
    })
}

/// The resolved id and, when the input was a prefix, the input.
pub(crate) fn resolve(
    snapshot: &dyn TaskSnapshotPort,
    raw: &str,
) -> Result<(TaskId, Option<String>), ReadError> {
    let id = valid_id(raw)?;
    let unknown = |reason| {
        ReadError::Resolution(Resolution::Unknown {
            input: raw.to_string(),
            reason,
        })
    };
    match snapshot.lookup(&id)? {
        Lookup::Found(found) => {
            let from = (found != id).then(|| raw.to_string());
            Ok((found, from))
        }
        Lookup::Pending(pending) => Err(ReadError::PendingLaunch {
            raw: raw.to_string(),
            state: pending.state,
            not_started_reason: pending.not_started_reason,
        }),
        Lookup::Missing => Err(ReadError::Resolution(missing_resolution(&id))),
        Lookup::InvalidPrefix => Err(unknown(Some(UnknownReason::PrefixTooShort))),
        Lookup::Incomplete => Err(unknown(Some(UnknownReason::PrefixIndexUnavailable))),
        Lookup::Ambiguous {
            candidates,
            truncated,
        } => {
            let found = snapshot.summaries(&candidates)?;
            Err(ReadError::Resolution(Resolution::Ambiguous {
                input: raw.to_string(),
                candidates: found
                    .into_iter()
                    .map(|task| Candidate {
                        task_id: task.id.as_str().to_string(),
                        title: task.title,
                        status: task.status,
                        created_at: task.created_at,
                    })
                    .collect(),
                truncated,
            }))
        }
    }
}
