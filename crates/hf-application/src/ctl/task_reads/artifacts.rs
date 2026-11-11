//! The artifact page read and the artifact summary: caller authority, ancestry, the
//! manifest, the cursor and the observation of each reference's target.
use super::{
    ReadError, Reads, Resolution, TaskSnapshotPort, UnknownReason,
    gate::{Entry, Family, Observer, enter_checked, guard, timely},
    history_ports::ArtifactTargetPort,
    resolve::{drift, resolve, valid_id},
};
use hf_domain::{
    ctl::{
        caller::{EnvMarkers, GatePath},
        history::{ArtifactItem, ArtifactSummary, Availability},
        state_root::StateRoot,
        task_rows::TaskFacts,
    },
    task_read::TaskId,
};

const LIMIT_MAX: i64 = 50;
const ANCESTORS_MAX: usize = 100;
const BUILDER: &str = "Managed workflow artifact access is not supported";
/// A page document is at most this many bytes under `ensure_ascii` sizing.
pub const PAGE_BYTES: usize = 63_000;

/// Where a page continues: the task, the manifest revision and the item offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NextCursor {
    pub task: String,
    pub revision: String,
    pub offset: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArtifactCursorArg {
    Absent,
    Malformed,
    Valid(NextCursor),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactRequest {
    pub task_id: String,
    pub limit: i64,
    pub cursor: ArtifactCursorArg,
    /// A non-empty `HARFLOW_ROOT_TASK_ID`: no caller can match it, so the read is refused.
    pub root_task_marker: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactEntry {
    pub item: ArtifactItem,
    pub availability: Availability,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactPage {
    pub task: TaskId,
    pub resolved_from: Option<String>,
    /// False when the manifest cannot be read: no items, count 0.
    pub readable: bool,
    pub count: usize,
    pub entries: Vec<ArtifactEntry>,
    pub next: Option<NextCursor>,
    pub has_more: bool,
}

fn refused(message: &str) -> ReadError {
    ReadError::InvalidParams(message.to_string())
}

/// Walks `continuation_of` through the facts alone, so a large header on an ancestor cannot
/// refuse the read. A missing or cyclic link, more than 100 tasks, or a workflow builder
/// anywhere in the chain is the message.
pub(crate) fn ancestry(
    snapshot: &dyn TaskSnapshotPort,
    facts: &TaskFacts,
) -> Result<Option<&'static str>, ReadError> {
    let mut seen = vec![facts.id.clone()];
    let mut next = facts.continuation_of.clone();
    if snapshot.workflow_builder(&facts.id)? {
        return Ok(Some(BUILDER));
    }
    while let Some(link) = next {
        if seen.contains(&link) {
            return Ok(Some("Task ancestry is cyclic"));
        }
        let Some(parent) = snapshot.continuation_link(&link)? else {
            return Ok(Some("Task ancestry is unavailable"));
        };
        if snapshot.workflow_builder(&link)? {
            return Ok(Some(BUILDER));
        }
        seen.push(link);
        if seen.len() > ANCESTORS_MAX {
            return Ok(Some("Task ancestry exceeds authority bounds"));
        }
        next = parent;
    }
    Ok(None)
}

/// `artifact_summary` of a task document. Broken ancestry offers no detail reference; an
/// unreadable manifest keeps one, so the detail read can say why.
pub(crate) fn summary(
    snapshot: &dyn TaskSnapshotPort,
    facts: &TaskFacts,
) -> Result<ArtifactSummary, ReadError> {
    if ancestry(snapshot, facts)?.is_some() {
        return Ok(ArtifactSummary {
            count: 0,
            available: false,
            detail_ref: false,
        });
    }
    let (count, available) = match snapshot.artifact_manifest(&facts.id)? {
        None => (0, true),
        Some(head) if head.readable() => (head.count, true),
        Some(_) => (0, false),
    };
    Ok(ArtifactSummary {
        count,
        available,
        detail_ref: true,
    })
}

/// A value that names no task, or only a short one, is the same "unknown task" error as an
/// absent canonical id; ambiguity and real prefix failures keep their own codes.
fn not_found(error: ReadError) -> ReadError {
    match error {
        ReadError::Resolution(Resolution::Unknown {
            reason: None | Some(UnknownReason::PrefixTooShort),
            ..
        })
        | ReadError::PendingLaunch { .. } => refused("Unknown task"),
        other => other,
    }
}

/// One page of a task's artifact references. `size` is the serialized size of a candidate
/// page of entries of the resolved task, against `PAGE_BYTES`. The gate is the bounded caller family; every
/// refusal is an artifact error, as is a root marker that no caller can match.
pub fn artifacts(
    reads: &Reads<'_>,
    targets: &dyn ArtifactTargetPort,
    root: &StateRoot,
    request: &ArtifactRequest,
    size: &dyn Fn(&str, &[ArtifactEntry]) -> usize,
) -> Result<ArtifactPage, ReadError> {
    let markers = if request.root_task_marker {
        EnvMarkers {
            present: true,
            non_empty: true,
        }
    } else {
        reads.markers
    };
    let gated = Reads {
        roots: reads.roots,
        store: reads.store,
        process: reads.process,
        markers,
        deadline: reads.deadline,
    };
    let syntax = || valid_id(&request.task_id).map(|_| ());
    let (marker, view) = match enter_checked(&gated, root, GatePath::Artifacts, &syntax)? {
        Entry::NoStore => return Err(refused("Unknown task")),
        Entry::Store { marker, view } => (marker, view),
    };
    let mut observer = Observer::new(gated.process);
    let mut out = None;
    gated
        .store
        .with_snapshot(root, gated.deadline, &mut |snapshot| {
            let gate = GatePath::Artifacts;
            guard(
                gate,
                snapshot,
                marker,
                &view,
                Family::Bounded,
                &mut observer,
            )?;
            out = Some(read_page(snapshot, targets, &gated, request, size)?);
            Ok(())
        })?;
    out.ok_or(ReadError::Internal)
}

fn read_page(
    snapshot: &dyn TaskSnapshotPort,
    targets: &dyn ArtifactTargetPort,
    reads: &Reads<'_>,
    request: &ArtifactRequest,
    size: &dyn Fn(&str, &[ArtifactEntry]) -> usize,
) -> Result<ArtifactPage, ReadError> {
    let (task, resolved_from) = resolve(snapshot, &request.task_id).map_err(not_found)?;
    let limit = usize::try_from(request.limit)
        .ok()
        .filter(|_| (1..=LIMIT_MAX).contains(&request.limit))
        .ok_or_else(|| refused("Artifact limit must be between 1 and 50"))?;
    let record = snapshot.record(&task)?.ok_or_else(drift)?;
    if let Some(message) = ancestry(snapshot, &record.facts)? {
        return Err(refused(message));
    }
    let empty = |readable: bool| ArtifactPage {
        task: task.clone(),
        resolved_from: resolved_from.clone(),
        readable,
        count: 0,
        entries: Vec::new(),
        next: None,
        has_more: false,
    };
    let head = match snapshot.artifact_manifest(&task)? {
        Some(head) if !head.readable() => return Ok(empty(false)),
        Some(head) => head,
        None if request.cursor == ArtifactCursorArg::Absent => return Ok(empty(true)),
        None => return Err(refused("Invalid or stale artifact cursor")),
    };
    let offset = match &request.cursor {
        ArtifactCursorArg::Absent => 0,
        ArtifactCursorArg::Valid(cursor)
            if cursor.task == task.as_str() && cursor.revision == head.revision =>
        {
            usize::try_from(cursor.offset)
                .ok()
                .filter(|offset| *offset <= head.count)
                .ok_or_else(|| refused("Invalid or stale artifact cursor"))?
        }
        _ => return Err(refused("Invalid or stale artifact cursor")),
    };
    let mut entries: Vec<ArtifactEntry> = Vec::new();
    for item in snapshot.artifact_items(&task, offset, limit)? {
        timely(reads.deadline)?;
        let availability = targets.availability(&record.facts.repo_path, &item);
        entries.push(ArtifactEntry { item, availability });
        if size(task.as_str(), &entries) > PAGE_BYTES {
            entries.pop();
            break;
        }
    }
    if let Some(message) = targets.command_failure() {
        return Err(refused(&message));
    }
    let end = offset + entries.len();
    let has_more = end < head.count;
    let next = has_more.then(|| NextCursor {
        task: task.as_str().to_string(),
        revision: head.revision.clone(),
        offset: i64::try_from(end).unwrap_or(i64::MAX),
    });
    Ok(ArtifactPage {
        task,
        resolved_from,
        readable: true,
        count: head.count,
        entries,
        next,
        has_more,
    })
}
