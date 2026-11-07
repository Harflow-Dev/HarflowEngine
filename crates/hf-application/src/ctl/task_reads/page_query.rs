//! The keyset walk of the listing page: filters, the byte cap, the cursor and the counts.
use super::{
    ReadError, Reads, TaskSnapshotPort, TaskView,
    gate::{Observer, timely},
    page::{CursorArg, ItemSize, PageOut, PageRequest, bad_cursor, limit_range},
    page_parts::{ROW_BYTES, counts, related},
};
use hf_domain::{
    ctl::task_rows::{BatchQuery, WireCursor},
    task_read::TaskId,
};

const BATCH_ROWS: usize = 100;
const SESSION_MAX: usize = 256;

pub(super) struct Query<'a> {
    request: &'a PageRequest,
    limit: usize,
    predecessor: Option<TaskId>,
}

impl<'a> Query<'a> {
    pub(super) fn new(
        request: &'a PageRequest,
        predecessor: Option<TaskId>,
    ) -> Result<Self, ReadError> {
        Ok(Self {
            request,
            limit: limit_range(request.limit)?,
            predecessor,
        })
    }

    pub(super) fn run(
        &self,
        snapshot: &dyn TaskSnapshotPort,
        observer: &mut Observer<'_>,
        reads: &Reads<'_>,
        size: ItemSize<'_>,
    ) -> Result<PageOut, ReadError> {
        let request = self.request;
        let session = request.session.as_deref();
        if session.is_some_and(|text| text.chars().count() > SESSION_MAX) {
            return Err(ReadError::InvalidParams(
                "session_id must be a bounded string".to_string(),
            ));
        }
        let scope =
            snapshot.cursor_scope(request.active_only, session, self.predecessor.as_ref())?;
        let mut after = match &request.cursor {
            CursorArg::Absent => None,
            CursorArg::Valid(cursor) if cursor.scope == scope => {
                Some((cursor.stamp, cursor.id.clone()))
            }
            _ => return Err(bad_cursor()),
        };
        let mut items: Vec<TaskView> = Vec::new();
        let (mut bytes, mut more) = (512usize, false);
        'walk: loop {
            timely(reads.deadline)?;
            let batch = snapshot.batch(&BatchQuery {
                after: after.clone(),
                session: request.session.clone(),
                predecessor: self.predecessor.clone(),
                since_us: None,
                unobserved_only: request.active_only,
                max_rows: BATCH_ROWS,
            })?;
            let Some(last) = batch.last() else { break };
            after = Some((last.facts.started_at_us, last.facts.id.as_str().to_string()));
            for record in batch {
                let view = observer.view(record);
                if request.active_only && !view.projection.active {
                    continue;
                }
                let weight = size(&view);
                if items.len() == self.limit || (!items.is_empty() && bytes + weight > ROW_BYTES) {
                    more = true;
                    break 'walk;
                }
                bytes += weight;
                items.push(view);
            }
        }
        let next = items.last().filter(|_| more).map(|last| WireCursor {
            scope: scope.clone(),
            stamp: last.record.facts.started_at_us,
            id: last.record.facts.id.as_str().to_string(),
        });
        let related = related(snapshot, observer, &items, size)?;
        let counts = counts(snapshot, observer, session, reads.deadline)?;
        Ok(PageOut {
            items,
            related,
            next,
            has_more: more,
            counts: Some(counts),
            ..PageOut::default()
        })
    }
}
