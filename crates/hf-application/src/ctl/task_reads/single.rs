//! `status` and `status-batch`.
use super::{
    ReadError, Reads, TaskView,
    artifacts::summary,
    gate::{Entry, Family, Observer, enter_checked, guard},
    resolve::{drift, missing_resolution, resolve, valid_id},
};
use hf_domain::ctl::{
    caller::GatePath, continuation_stop::ContinuationStop, history::ArtifactSummary,
    state_root::StateRoot,
};

const BATCH_MAX: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusOut {
    pub view: TaskView,
    /// The input, when it was a prefix.
    pub resolved_from: Option<String>,
    pub artifacts: ArtifactSummary,
    /// Whether the task's chain was stopped, read in the same snapshot as the record.
    pub stop: ContinuationStop,
    /// For `--brief`: stored event bodies from the log's last megabyte, oldest first.
    pub recent_bodies: Option<Vec<Vec<u8>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchRow {
    pub view: TaskView,
    pub resolved_from: Option<String>,
    pub artifacts: ArtifactSummary,
    pub stop: ContinuationStop,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BatchOut {
    pub rows: Vec<BatchRow>,
    /// Keyed by the raw input, in the order they arose.
    pub errors: Vec<(String, ReadError)>,
}

fn vanished(id: &hf_domain::task_read::TaskId) -> ReadError {
    ReadError::Resolution(missing_resolution(id))
}

pub fn status(reads: &Reads<'_>, root: &StateRoot, raw: &str) -> Result<StatusOut, ReadError> {
    status_with(reads, root, raw, false)
}

/// As `status`; with `brief` the recent event bodies are read in the same snapshot.
pub fn status_with(
    reads: &Reads<'_>,
    root: &StateRoot,
    raw: &str,
    brief: bool,
) -> Result<StatusOut, ReadError> {
    let syntax = || valid_id(raw).map(|_| ());
    let (marker, view) = match enter_checked(reads, root, GatePath::Managed, &syntax)? {
        Entry::NoStore => return Err(vanished(&valid_id(raw)?)),
        Entry::Store { marker, view } => (marker, view),
    };
    let mut observer = Observer::new(reads.process);
    let mut out = None;
    reads
        .store
        .with_snapshot(root, reads.deadline, &mut |snapshot| {
            guard(
                GatePath::Managed,
                snapshot,
                marker,
                &view,
                Family::Complete,
                &mut observer,
            )?;
            let (found, resolved_from) = resolve(snapshot, raw)?;
            let (record, row) = snapshot.record_with_stop(&found)?.ok_or_else(drift)?;
            let stop = ContinuationStop::observe(&record.facts.id, &record.facts.chain_root, &row);
            let artifacts = summary(snapshot, &record.facts)?;
            let recent_bodies = if brief {
                Some(snapshot.recent_event_bodies(&found)?)
            } else {
                None
            };
            out = Some(StatusOut {
                view: observer.view(record),
                resolved_from,
                artifacts,
                stop,
                recent_bodies,
            });
            Ok(())
        })?;
    out.ok_or(ReadError::Internal)
}

pub fn status_batch(
    reads: &Reads<'_>,
    root: &StateRoot,
    raw_ids: &[String],
) -> Result<BatchOut, ReadError> {
    let mut ids: Vec<&String> = Vec::new();
    for raw in raw_ids {
        if !ids.contains(&raw) {
            ids.push(raw);
        }
    }
    let bounds = || {
        if ids.is_empty() || ids.len() > BATCH_MAX {
            let message = format!(
                "status-batch takes 1..{BATCH_MAX} comma-separated task ids, got {}",
                ids.len()
            );
            return Err(ReadError::InvalidParams(message));
        }
        Ok(())
    };
    let mut out = BatchOut::default();
    let mut valid = Vec::new();
    for raw in ids.iter().copied() {
        match valid_id(raw) {
            Ok(id) => valid.push((raw.as_str(), id)),
            Err(error) => out.errors.push((raw.clone(), error)),
        }
    }
    let (marker, view) = match enter_checked(reads, root, GatePath::Managed, &bounds)? {
        Entry::NoStore => {
            for (raw, id) in valid {
                out.errors.push((
                    raw.to_string(),
                    ReadError::Resolution(missing_resolution(&id)),
                ));
            }
            return Ok(out);
        }
        Entry::Store { marker, view } => (marker, view),
    };
    let mut observer = Observer::new(reads.process);
    let mut result = None;
    reads
        .store
        .with_snapshot(root, reads.deadline, &mut |snapshot| {
            guard(
                GatePath::Managed,
                snapshot,
                marker,
                &view,
                Family::Complete,
                &mut observer,
            )?;
            let mut batch = BatchOut {
                rows: Vec::new(),
                errors: out.errors.clone(),
            };
            for (raw, _) in &valid {
                let found = resolve(snapshot, raw).and_then(|(id, from)| {
                    let (record, row) = snapshot.record_with_stop(&id)?.ok_or_else(drift)?;
                    let stop =
                        ContinuationStop::observe(&record.facts.id, &record.facts.chain_root, &row);
                    let artifacts = summary(snapshot, &record.facts)?;
                    Ok((record, from, artifacts, stop))
                });
                match found {
                    Ok((record, resolved_from, artifacts, stop)) => batch.rows.push(BatchRow {
                        view: observer.view(record),
                        resolved_from,
                        artifacts,
                        stop,
                    }),
                    Err(error @ ReadError::Store(_)) => return Err(error),
                    Err(error) => batch.errors.push((raw.to_string(), error)),
                }
            }
            result = Some(batch);
            Ok(())
        })?;
    result.ok_or(ReadError::Internal)
}
