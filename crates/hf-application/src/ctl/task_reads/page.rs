//! The page read: plain, explicit ids and successors of one task.
use super::{
    ReadError, Reads, Resolution, TaskSnapshotPort, TaskView,
    gate::{Entry, Family, Observer, authorize, check_damage, enter},
    page_parts::related,
    page_query::Query,
    resolve::{drift, missing_resolution, resolve, valid_id},
};
use hf_domain::{
    ctl::{
        caller::{GatePath, marker_refusal},
        state_root::StateRoot,
        task_rows::WireCursor,
        text::py_repr,
    },
    task_read::TaskId,
};

const PAGE_MAX: i64 = 100;
const IDS_MAX: usize = 100;
const BLOCKED_REASON: &str = "Metadata or caller identity requires direct inspection";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CursorArg {
    Absent,
    Valid(WireCursor),
    Malformed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageMode {
    Plain,
    /// The raw comma-separated entries.
    TaskIds(Vec<String>),
    /// The raw predecessor id or prefix.
    Successors(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageRequest {
    pub mode: PageMode,
    pub limit: i64,
    pub cursor: CursorArg,
    pub active_only: bool,
    pub session: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counts {
    pub complete: bool,
    pub active: Option<u64>,
    pub active_root: Option<u64>,
    pub attention_root: Option<u64>,
}

impl Counts {
    pub(crate) fn unknown() -> Self {
        Self {
            complete: false,
            active: None,
            active_root: None,
            attention_root: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageOut {
    pub items: Vec<TaskView>,
    pub related: Vec<TaskView>,
    pub next: Option<WireCursor>,
    pub has_more: bool,
    /// Absent for an explicit-id page, which carries no counts.
    pub counts: Option<Counts>,
    /// `(input, id)` of the explicit ids that were prefixes.
    pub resolved_task_ids: Vec<(String, String)>,
    /// The resolved predecessor and, when the input was a prefix, the input.
    pub predecessor: Option<(String, Option<String>)>,
    /// The reason an incomplete history withholds every item.
    pub withheld: Option<String>,
}

impl PageOut {
    fn withheld() -> Self {
        Self {
            withheld: Some(BLOCKED_REASON.to_string()),
            ..Self::default()
        }
    }
}

fn bad_limit() -> ReadError {
    ReadError::InvalidParams("limit must be 1-100".to_string())
}

pub(super) fn bad_cursor() -> ReadError {
    ReadError::InvalidParams("Invalid listing cursor".to_string())
}

pub(super) fn limit_range(limit: i64) -> Result<usize, ReadError> {
    if (1..=PAGE_MAX).contains(&limit) {
        usize::try_from(limit).map_err(|_| bad_limit())
    } else {
        Err(bad_limit())
    }
}

/// How many bytes a rendered item takes against the page budget.
pub type ItemSize<'a> = &'a dyn Fn(&TaskView) -> usize;

/// The page read, with the page's byte budget measured by `size`, the renderer's own size.
pub fn page_with(
    reads: &Reads<'_>,
    root: &StateRoot,
    request: &PageRequest,
    size: ItemSize<'_>,
) -> Result<PageOut, ReadError> {
    match &request.mode {
        PageMode::Successors(raw) => successors(reads, root, request, raw, size),
        PageMode::Plain => listing(reads, root, request, None, size),
        PageMode::TaskIds(raw) => listing(reads, root, request, Some(raw), size),
    }
}

fn listing(
    reads: &Reads<'_>,
    root: &StateRoot,
    request: &PageRequest,
    ids: Option<&Vec<String>>,
    size: ItemSize<'_>,
) -> Result<PageOut, ReadError> {
    let (marker, view) = match enter(reads, root, GatePath::Page)? {
        Entry::NoStore => return empty_root(request, ids),
        Entry::Store { marker, view } => (marker, view),
    };
    let mut observer = Observer::new(reads.process);
    let mut out = None;
    reads
        .store
        .with_snapshot(root, reads.deadline, &mut |snapshot| {
            // A damaged history is a withheld page, and it outranks a marker.
            match check_damage(snapshot) {
                Ok(()) => {}
                Err(ReadError::Damaged) => {
                    out = Some(PageOut::withheld());
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
            authorize(
                GatePath::Page,
                snapshot,
                marker,
                &view,
                Family::Bounded,
                &mut observer,
            )?;
            out = Some(match ids {
                Some(raw) => explicit(snapshot, &mut observer, raw, size)?,
                None => {
                    let query = Query::new(request, None)?;
                    query.run(snapshot, &mut observer, reads, size)?
                }
            });
            Ok(())
        })?;
    out.ok_or(ReadError::Internal)
}

/// A root that holds no store: an empty, complete page, or the explicit ids' answers.
fn empty_root(request: &PageRequest, ids: Option<&Vec<String>>) -> Result<PageOut, ReadError> {
    if let Some(raw) = ids {
        for_each_id(raw, |_, id| {
            if id.canonical() {
                Ok(())
            } else {
                Err(ReadError::Resolution(missing_resolution(&id)))
            }
        })?;
        return Ok(PageOut::default());
    }
    limit_range(request.limit)?;
    if request.cursor != CursorArg::Absent {
        return Err(bad_cursor());
    }
    Ok(PageOut {
        counts: Some(Counts {
            complete: true,
            active: Some(0),
            active_root: Some(0),
            attention_root: Some(0),
        }),
        ..PageOut::default()
    })
}

/// Valid entries are handled in request order, so the first one that names nothing answers
/// before a malformed entry or the count is reported, as the reference does.
fn for_each_id(
    raw: &[String],
    mut each: impl FnMut(&str, TaskId) -> Result<(), ReadError>,
) -> Result<(), ReadError> {
    let mut invalid = None;
    for entry in raw {
        match valid_id(entry) {
            Ok(id) => each(entry, id)?,
            Err(_) => invalid = invalid.or(Some(entry)),
        }
    }
    if let Some(entry) = invalid {
        return Err(ReadError::InvalidParams(format!(
            "not a valid task id: {}",
            py_repr(entry)
        )));
    }
    if raw.len() > IDS_MAX {
        return Err(ReadError::InvalidParams(format!(
            "task-list-page takes at most {IDS_MAX} task ids"
        )));
    }
    Ok(())
}

/// Explicit ids ignore the limit and the cursor and keep request order. A missing canonical
/// id is dropped silently; any other value that names no task is reported, in request order.
fn explicit(
    snapshot: &dyn TaskSnapshotPort,
    observer: &mut Observer<'_>,
    raw: &[String],
    size: ItemSize<'_>,
) -> Result<PageOut, ReadError> {
    let mut found: Vec<TaskId> = Vec::new();
    let mut echoes = Vec::new();
    for_each_id(raw, |entry, _| {
        match resolve(snapshot, entry) {
            Ok((id, from)) => {
                if let Some(input) = from {
                    echoes.push((input, id.as_str().to_string()));
                }
                if !found.contains(&id) {
                    found.push(id);
                }
            }
            Err(ReadError::PendingLaunch(_)) => {}
            Err(ReadError::Resolution(Resolution::Unknown { reason: None, .. })) => {}
            Err(error) => return Err(error),
        }
        Ok(())
    })?;
    let mut items = Vec::new();
    for id in &found {
        // The lookup found it in this snapshot, so a missing record is a store fault.
        let record = snapshot.record(id)?.ok_or_else(drift)?;
        items.push(observer.view(record));
    }
    let related = related(snapshot, observer, &items, size)?;
    Ok(PageOut {
        items,
        related,
        resolved_task_ids: echoes,
        ..PageOut::default()
    })
}

fn successors(
    reads: &Reads<'_>,
    root: &StateRoot,
    request: &PageRequest,
    raw: &str,
    size: ItemSize<'_>,
) -> Result<PageOut, ReadError> {
    if let Some(kind) = marker_refusal(GatePath::Successors, reads.markers) {
        return Err(ReadError::Refused(kind));
    }
    let (marker, view) = match enter(reads, root, GatePath::Successors)? {
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
                GatePath::Successors,
                snapshot,
                marker,
                &view,
                Family::Complete,
                &mut observer,
            )?;
            let (predecessor, from) = resolve(snapshot, raw)?;
            let query = Query::new(request, Some(predecessor.clone()))?;
            let mut page = query.run(snapshot, &mut observer, reads, size)?;
            page.predecessor = Some((predecessor.as_str().to_string(), from));
            out = Some(page);
            Ok(())
        })?;
    out.ok_or(ReadError::Internal)
}
