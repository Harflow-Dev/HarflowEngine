//! The direct-only history reads: the gate they share and the prompt read.
use super::{
    ReadError, Reads, TaskSnapshotPort,
    gate::{Entry, Family, Observer, authorize, check_damage, enter},
    resolve::{drift, missing_resolution, resolve, valid_id},
};
use hf_domain::{
    ctl::{
        caller::{GatePath, marker_refusal},
        history::{PromptView, view_prompt},
        state_root::StateRoot,
    },
    task_read::TaskId,
};

/// What `read` receives: the snapshot, the resolved id and, for a prefix, the input.
pub(crate) type DirectRead<'a, T> =
    &'a mut dyn FnMut(&dyn TaskSnapshotPort, &TaskId, Option<String>) -> Result<T, ReadError>;

/// Runs `read` in one snapshot behind the direct-only gate: markers by presence before the
/// store, then the root class and the process view, then in the snapshot the damage check,
/// complete caller detection and id resolution. Nothing about any task is read before the
/// gate, so a refused caller learns nothing about which ids exist.
pub(crate) fn direct_read<T>(
    reads: &Reads<'_>,
    root: &StateRoot,
    raw: &str,
    read: DirectRead<'_, T>,
) -> Result<T, ReadError> {
    if let Some(kind) = marker_refusal(GatePath::DirectOnly, reads.markers) {
        return Err(ReadError::Refused(kind));
    }
    let (marker, view) = match enter(reads, root, GatePath::DirectOnly)? {
        Entry::NoStore => return Err(ReadError::Resolution(missing_resolution(&valid_id(raw)?))),
        Entry::Store { marker, view } => (marker, view),
    };
    let mut observer = Observer::new(reads.process);
    let mut out = None;
    reads
        .store
        .with_snapshot(root, reads.deadline, &mut |snapshot| {
            check_damage(snapshot)?;
            authorize(
                GatePath::DirectOnly,
                snapshot,
                marker,
                &view,
                Family::Complete,
                &mut observer,
            )?;
            let (id, from) = resolve(snapshot, raw)?;
            out = Some(read(snapshot, &id, from)?);
            Ok(())
        })?;
    out.ok_or(ReadError::Internal)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptOut {
    pub task: TaskId,
    pub resolved_from: Option<String>,
    pub title: Option<String>,
    pub view: PromptView,
}

/// One task's stored prompt, cut to `max_bytes` of text.
pub fn prompt(
    reads: &Reads<'_>,
    root: &StateRoot,
    raw: &str,
    max_bytes: usize,
) -> Result<PromptOut, ReadError> {
    direct_read(reads, root, raw, &mut |snapshot, id, resolved_from| {
        let stored = snapshot.prompt(id, max_bytes)?.ok_or_else(drift)?;
        let view = view_prompt(&stored, max_bytes);
        Ok(PromptOut {
            task: id.clone(),
            resolved_from,
            title: stored.title,
            view,
        })
    })
}
