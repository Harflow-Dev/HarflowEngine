//! The page of `task-pending-page`: rows in receipt filename order, resumed by the same key.
use super::{
    NamespaceFailure, PendingBudget, PendingLimits, ReadError, Reads, StoreFailure,
    TaskSnapshotPort,
    pending::{Seen, failure, gated, read_candidate, unavailable},
};
use hf_domain::ctl::{
    pending::{
        ITEMS_RESERVE, LaunchFacts, LaunchOutcome, LaunchView, MAX_EXAMINED, MAX_LIMIT,
        MAX_RESPONSE_BYTES, outcome,
    },
    state_root::StateRoot,
};
use hf_domain::task_read::TaskId;

/// The position a page resumes from, and what it is bound to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingCursor {
    pub root: String,
    pub identity: [String; 4],
    pub revision: String,
    /// The task identity of the last candidate examined.
    pub last: String,
    pub limit: i64,
    pub uncertain: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingCursorArg {
    Absent,
    /// Present, and not a cursor of this family.
    Malformed,
    Valid(PendingCursor),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingPageRequest {
    pub limit: i64,
    pub cursor: PendingCursorArg,
}

/// One row a page lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingItem {
    Pending {
        id: String,
        view: Box<LaunchView>,
    },
    Refused {
        id: String,
        code: &'static str,
        check: &'static str,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingPageOut {
    pub items: Vec<PendingItem>,
    pub namespace_revision: String,
    pub invalid_name_count: u64,
    pub examined_count: usize,
    pub has_more: bool,
    pub coverage_complete: bool,
    pub next: Option<PendingCursor>,
    pub bound: Option<&'static str>,
}

/// What the loop over the candidates produced.
struct Walk {
    items: Vec<PendingItem>,
    last: Option<String>,
    examined: usize,
    bound: Option<&'static str>,
    uncertain: bool,
    /// A candidate remains after the last one examined.
    has_more: bool,
    /// The serialized size of the listed items, with the separators of a list.
    list_bytes: usize,
}

fn item_of(facts: &LaunchFacts) -> Option<PendingItem> {
    let id = facts.id.clone();
    match outcome(facts) {
        LaunchOutcome::Pending(view) => Some(PendingItem::Pending { id, view }),
        LaunchOutcome::Refused { code, check } => Some(PendingItem::Refused { id, code, check }),
        LaunchOutcome::Recorded | LaunchOutcome::Inconsistent => None,
    }
}

/// Examines candidates in order, one row at a time, until the page is full or a bound
/// stops it; no later receipt is read. Filtered candidates count as examined and move the
/// cursor forward.
fn walk(
    snapshot: &dyn TaskSnapshotPort,
    reads: &Reads<'_>,
    budget: &PendingBudget,
    request: &PendingPageRequest,
    start: (Option<String>, bool),
    size: &dyn Fn(&PendingItem) -> usize,
) -> Result<Walk, ReadError> {
    let limit = usize::try_from(request.limit).unwrap_or(usize::MAX);
    let source = unavailable("source_unavailable", reads.deadline);
    let mut state = Walk {
        items: Vec::new(),
        last: start.0,
        examined: 0,
        bound: None,
        uncertain: start.1,
        has_more: false,
        list_bytes: 2,
    };
    loop {
        let Some(name) = snapshot
            .next_launch_name(state.last.as_deref())
            .map_err(&source)?
        else {
            return Ok(state);
        };
        state.has_more = true;
        if state.examined >= MAX_EXAMINED || state.items.len() >= limit {
            return Ok(state);
        }
        let id = TaskId::new(name)
            .map_err(|_| failure("pending_observation_unavailable", "source_unavailable"))?;
        let item = match read_candidate(snapshot, &id, (budget, reads.deadline), true)? {
            Seen::OutOfBudget => {
                state.bound = Some("read_budget");
                return Ok(state);
            }
            Seen::Missing => {
                // The receipt vanished between the two reads: it is not part of this page.
                state.last = Some(id.as_str().to_owned());
                state.examined += 1;
                state.uncertain = true;
                continue;
            }
            Seen::Recorded => None,
            Seen::Launch(facts) => item_of(&facts),
        };
        if let Some(item) = &item {
            let separator = if state.items.is_empty() { 0 } else { 2 };
            let total = state.list_bytes + separator + size(item);
            if total > MAX_RESPONSE_BYTES - ITEMS_RESERVE {
                state.bound = Some("response_bytes");
                return Ok(state);
            }
            state.list_bytes = total;
            state.uncertain |= matches!(item, PendingItem::Refused { .. });
        }
        state.items.extend(item);
        state.last = Some(id.as_str().to_owned());
        state.examined += 1;
        state.has_more = false;
    }
}

/// The cursor of the request, checked against the limit and the root it was issued for.
fn prior_of(request: &PendingPageRequest, root: &str) -> Result<Option<PendingCursor>, ReadError> {
    match &request.cursor {
        PendingCursorArg::Absent => Ok(None),
        PendingCursorArg::Malformed => Err(failure("pending_cursor_invalid", "cursor_shape")),
        PendingCursorArg::Valid(prior) => {
            if prior.root != root || prior.limit != request.limit {
                return Err(failure("pending_cursor_invalid", "cursor_scope"));
            }
            Ok(Some(prior.clone()))
        }
    }
}

fn namespace_error(
    failure_kind: NamespaceFailure,
    unavailable: &dyn Fn(StoreFailure) -> ReadError,
) -> ReadError {
    match failure_kind {
        NamespaceFailure::Bound => failure("pending_observation_bound", "namespace_bound"),
        NamespaceFailure::Store(store) => unavailable(store),
    }
}

fn page_in(
    reads: &Reads<'_>,
    root: &StateRoot,
    snapshot: &dyn TaskSnapshotPort,
    budget: &PendingBudget,
    request: &PendingPageRequest,
    (limits, size): (&PendingLimits, &dyn Fn(&PendingItem) -> usize),
) -> Result<PendingPageOut, ReadError> {
    if !(1..=MAX_LIMIT).contains(&request.limit) {
        return Err(failure("invalid_pending_limit", "limit_1_to_100"));
    }
    let unavailable_ns = unavailable("namespace_unavailable", reads.deadline);
    let binding = snapshot
        .pending_binding(root.as_str())
        .map_err(&unavailable_ns)?;
    let prior = prior_of(request, &binding.root)?;
    let space = snapshot
        .pending_namespace(limits)
        .map_err(|error| namespace_error(error, &unavailable_ns))?;
    let identity = binding.identity.map(|part| part.to_string());
    if let Some(prior) = &prior {
        if prior.identity != identity || prior.revision != space.revision {
            return Err(failure("pending_cursor_expired", "namespace_changed"));
        }
        if !snapshot
            .launch_name_exists(&prior.last)
            .map_err(&unavailable_ns)?
        {
            return Err(failure("pending_cursor_invalid", "cursor_position"));
        }
    }
    let after = prior.as_ref().map(|prior| prior.last.clone());
    let uncertain = space.invalid > 0 || prior.as_ref().is_some_and(|prior| prior.uncertain);
    let done = walk(snapshot, reads, budget, request, (after, uncertain), size)?;
    if let (Some(bound), 0) = (done.bound, done.examined) {
        return Err(failure("pending_observation_bound", bound));
    }
    let has_more = done.has_more;
    let uncertain = done.uncertain || done.bound.is_some();
    let next = has_more.then(|| PendingCursor {
        root: binding.root.clone(),
        identity,
        revision: space.revision.clone(),
        last: done.last.clone().unwrap_or_default(),
        limit: request.limit,
        uncertain,
    });
    Ok(PendingPageOut {
        items: done.items,
        namespace_revision: space.revision,
        invalid_name_count: space.invalid,
        examined_count: done.examined,
        has_more,
        coverage_complete: !(uncertain || has_more),
        next,
        bound: done.bound,
    })
}

/// A page of pending launches. `size` is the serialized size of one listed item, which the
/// response bound is counted in.
pub fn pending_page(
    reads: &Reads<'_>,
    root: &StateRoot,
    request: &PendingPageRequest,
    limits: &PendingLimits,
    size: &dyn Fn(&PendingItem) -> usize,
) -> Result<PendingPageOut, ReadError> {
    gated(reads, root, limits, &mut |snapshot, budget| {
        page_in(reads, root, snapshot, budget, request, (limits, size))
    })
}
