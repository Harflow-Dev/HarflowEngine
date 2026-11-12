//! `recovery-activity`: the capture state of one task, which decides the page it gets.
use super::{
    ReadError, Reads,
    gate::{Entry, Family, Observer, enter_checked, guard},
    resolve::{missing_resolution, resolve, valid_id},
};
use hf_domain::{
    ctl::{caller::GatePath, recovery, state_root::StateRoot, text::py_repr},
    event_read::CaptureState,
    task_read::TaskId,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryOut {
    pub task: TaskId,
    pub resolved_from: Option<String>,
    pub backend: String,
    pub capture: CaptureState,
}

pub(super) fn unknown_task(raw: &str) -> ReadError {
    ReadError::Coded {
        code: "unknown_task",
        message: format!("unknown task_id: {raw}"),
    }
}

/// The reference looks a backend up only when capture loss is confirmed, and refuses a name it
/// does not register.
fn unregistered(backend: &str) -> ReadError {
    let names: Vec<String> = recovery::BACKENDS
        .iter()
        .map(|name| py_repr(name))
        .collect();
    ReadError::InvalidParams(format!(
        "unknown backend {}; expected one of [{}]",
        py_repr(backend),
        names.join(", ")
    ))
}

/// The path of `status`: a managed read, with the id syntax judged before any marker.
pub fn recovery_activity(
    reads: &Reads<'_>,
    root: &StateRoot,
    raw: &str,
) -> Result<RecoveryOut, ReadError> {
    let syntax = || valid_id(raw).map(|_| ());
    let (marker, view) = match enter_checked(reads, root, GatePath::Managed, &syntax)? {
        Entry::NoStore => return Err(ReadError::Resolution(missing_resolution(&valid_id(raw)?))),
        Entry::Store { marker, view } => (marker, view),
    };
    let mut observer = Observer::new(reads.process);
    let mut out = None;
    reads
        .store
        .with_snapshot(root, reads.deadline, &mut |snapshot| {
            let managed = (GatePath::Managed, Family::Complete);
            guard(managed.0, snapshot, marker, &view, managed.1, &mut observer)?;
            let (id, resolved_from) = resolve(snapshot, raw).map_err(|error| match error {
                ReadError::PendingLaunch { raw, .. } => unknown_task(&raw),
                ReadError::LaunchDocument { id, .. } => unknown_task(&id),
                other => other,
            })?;
            let record = snapshot
                .record(&id)?
                .ok_or_else(|| unknown_task(id.as_str()))?;
            let stored = snapshot.stream_capture(&id)?;
            let facts = &record.facts;
            let capture = recovery::capture(&facts.status, facts.exit_code, stored);
            let registered = recovery::BACKENDS.contains(&facts.backend.as_str());
            if capture == CaptureState::LostWithOwner && !registered {
                return Err(unregistered(&facts.backend));
            }
            out = Some(RecoveryOut {
                task: id,
                resolved_from,
                backend: facts.backend.clone(),
                capture,
            });
            Ok(())
        })?;
    out.ok_or(ReadError::Internal)
}
