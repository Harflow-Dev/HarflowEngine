//! Counts and related headers of a page.
use super::{
    ReadError, TaskSnapshotPort, TaskView,
    gate::{Observer, timely},
    page::{Counts, ItemSize},
};
use hf_domain::{ctl::task_rows::TaskRecord, task_read::TaskId};
use std::time::Instant;

/// Over this many unobserved rows root-wide, no count is claimed.
const UNOBSERVED_CAP: u64 = 4096;
const BATCH_ROWS: usize = 100;
/// A page holds at most 256 KiB: half for its rows, the rest for the related headers
/// (`catalog.py` `PAGE_BYTES` and `ROW_BYTES`).
pub(crate) const PAGE_BYTES: usize = 256 * 1024;
pub(crate) const ROW_BYTES: usize = 128 * 1024;
const RELATED_MAX: usize = 32;
const VISITS_MAX: usize = 132;

/// `active` counts live-projected active rows under the session filter alone. An active row
/// is an unobserved one that is not dead, so an uncertain row is active; if any such row
/// under the filter is uncertain the count is unknown. `attention` is always zero: an
/// active row projects to `running`.
pub(crate) fn counts(
    snapshot: &dyn TaskSnapshotPort,
    observer: &mut Observer<'_>,
    session: Option<&str>,
    deadline: Instant,
) -> Result<Counts, ReadError> {
    if snapshot.unobserved_count()? > UNOBSERVED_CAP {
        return Ok(Counts::unknown());
    }
    let (mut root, mut scoped, mut uncertain) = (0u64, 0u64, false);
    let mut after = None;
    loop {
        timely(deadline)?;
        let rows = snapshot.unobserved_rows(after.clone(), BATCH_ROWS)?;
        let Some(last) = rows.last() else { break };
        after = Some((last.stamp, last.id.as_str().to_string()));
        for row in rows {
            let verdict = observer.verdict(row.process.as_ref());
            let projection =
                hf_domain::ctl::task_projection::project(&row.status, row.exit_code, verdict);
            if !projection.active {
                continue;
            }
            root += 1;
            if session.is_none_or(|wanted| wanted == row.session) {
                scoped += 1;
                uncertain |= projection.identity_state == "uncertain";
            }
        }
    }
    Ok(Counts {
        complete: !uncertain,
        active: (!uncertain).then_some(scoped),
        active_root: Some(root),
        attention_root: Some(0),
    })
}

/// The tasks the items continue from, nearest first: 32 headers at most, 132 visits.
pub(crate) fn related(
    snapshot: &dyn TaskSnapshotPort,
    observer: &mut Observer<'_>,
    items: &[TaskView],
    size: ItemSize<'_>,
) -> Result<Vec<TaskView>, ReadError> {
    let mut bytes = 0usize;
    let mut visited: Vec<TaskId> = items
        .iter()
        .map(|item| item.record.facts.id.clone())
        .collect();
    let mut queue: Vec<TaskId> = Vec::new();
    for item in items {
        push_links(&mut queue, &item.record, &visited);
    }
    let mut found = Vec::new();
    while !queue.is_empty() && found.len() < RELATED_MAX && visited.len() < VISITS_MAX {
        let id = queue.remove(0);
        if visited.contains(&id) {
            continue;
        }
        visited.push(id.clone());
        let Some(record) = snapshot.record(&id)? else {
            continue;
        };
        let mut links = Vec::new();
        push_links(&mut links, &record, &visited);
        queue.splice(0..0, links);
        let view = observer.view(record);
        bytes += size(&view);
        // The related headers have the rest of the page, never more than the rows had.
        if bytes > PAGE_BYTES - ROW_BYTES {
            break;
        }
        found.push(view);
    }
    Ok(found)
}

fn push_links(queue: &mut Vec<TaskId>, record: &TaskRecord, visited: &[TaskId]) {
    let facts = &record.facts;
    for link in [&facts.continuation_of, &facts.chain_root]
        .into_iter()
        .flatten()
    {
        if !visited.contains(link) {
            queue.push(link.clone());
        }
    }
}
