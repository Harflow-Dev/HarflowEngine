//! `list`: every recorded task, newest first, in one snapshot.
use super::{
    ReadError, Reads, TaskView,
    gate::{Entry, Family, Observer, enter, guard, timely},
};
use hf_domain::ctl::{caller::GatePath, state_root::StateRoot, task_rows::BatchQuery};

const BATCH_ROWS: usize = 100;

/// `since_us` keeps only tasks started at or after that instant. There is no row cap; a
/// deadline overrun is `state_busy`, never a partial list.
pub fn list(
    reads: &Reads<'_>,
    root: &StateRoot,
    since_us: Option<i64>,
) -> Result<Vec<TaskView>, ReadError> {
    let (marker, view) = match enter(reads, root, GatePath::Managed)? {
        Entry::NoStore => return Ok(Vec::new()),
        Entry::Store { marker, view } => (marker, view),
    };
    let mut observer = Observer::new(reads.process);
    let mut tasks = Vec::new();
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
            tasks.clear();
            let mut after = None;
            loop {
                timely(reads.deadline)?;
                let batch = snapshot.batch(&BatchQuery {
                    after: after.clone(),
                    since_us,
                    max_rows: BATCH_ROWS,
                    ..BatchQuery::default()
                })?;
                let Some(last) = batch.last() else { break };
                after = Some((last.facts.started_at_us, last.facts.id.as_str().to_string()));
                tasks.extend(batch.into_iter().map(|record| observer.view(record)));
            }
            Ok(())
        })?;
    Ok(tasks)
}
