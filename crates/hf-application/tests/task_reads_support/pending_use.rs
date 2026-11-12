//! Shared builders of the pending read tests: launch facts and the two requests.
use super::{World, caller, identity};
use hf_application::ctl::task_reads::PendingFailure;
use hf_application::ctl::task_reads::{
    PendingCursor, PendingCursorArg, PendingItem, PendingLimits, PendingPageOut,
    PendingPageRequest, PendingStatusOut, ReadError, pending_page, pending_status,
};
use hf_domain::ctl::{
    caller::{EnvMarkers, PidObservation},
    pending::LaunchFacts,
};

/// An unrefused launch of `id` in `state`, with a stored size of 10 bytes.
pub fn pending(id: &str, state: &str) -> LaunchFacts {
    LaunchFacts {
        id: id.to_owned(),
        state: Some(state.to_owned()),
        generation: Some("g".into()),
        backend: Some("claude".into()),
        text_valid: true,
        ..LaunchFacts::default()
    }
}

/// A launch whose id also names an ordinary record.
pub fn recorded(id: &str) -> LaunchFacts {
    LaunchFacts {
        record: true,
        ..pending(id, "reserved")
    }
}

/// A receipt that does not qualify as a launch document.
pub fn refused(id: &str) -> LaunchFacts {
    LaunchFacts {
        id: id.to_owned(),
        refusal: Some(("format".into(), "format".into())),
        ..LaunchFacts::default()
    }
}

pub fn request(limit: i64, cursor: PendingCursorArg) -> PendingPageRequest {
    PendingPageRequest { limit, cursor }
}

pub fn small(_: &PendingItem) -> usize {
    100
}

pub fn page_with(
    world: &World,
    markers: EnvMarkers,
    request: &PendingPageRequest,
    limits: &PendingLimits,
    size: &dyn Fn(&PendingItem) -> usize,
) -> Result<PendingPageOut, ReadError> {
    let reads = world.reads(markers);
    pending_page(&reads, &super::root(), request, limits, size)
}

/// The first page of `limit` rows with default bounds and no markers.
pub fn first(world: &World, limit: i64) -> Result<PendingPageOut, ReadError> {
    page_with(
        world,
        super::NONE_MARKERS,
        &request(limit, PendingCursorArg::Absent),
        &PendingLimits::default(),
        &small,
    )
}

/// The page after `cursor`, with the same limit.
pub fn after(world: &World, cursor: &PendingCursor) -> Result<PendingPageOut, ReadError> {
    page_with(
        world,
        super::NONE_MARKERS,
        &request(cursor.limit, PendingCursorArg::Valid(cursor.clone())),
        &PendingLimits::default(),
        &small,
    )
}

pub fn status_of(world: &World, raw: &str) -> Result<PendingStatusOut, ReadError> {
    let reads = world.reads(super::NONE_MARKERS);
    pending_status(&reads, &super::root(), raw, &PendingLimits::default())
}

pub fn ids(out: &PendingPageOut) -> Vec<String> {
    out.items
        .iter()
        .map(|item| match item {
            PendingItem::Pending { id, .. } | PendingItem::Refused { id, .. } => id.clone(),
        })
        .collect()
}

pub fn check(code: &'static str, check: &'static str) -> ReadError {
    ReadError::Pending(PendingFailure::Check { code, check })
}

pub fn undecidable() -> ReadError {
    check("caller_undecidable", "direct_caller_required")
}

pub fn agent() -> ReadError {
    check("agent_caller", "direct_caller_required")
}

pub fn world() -> World {
    let mut world = World::new();
    world.pending_world.rows.push(pending("ab", "reserved"));
    world
}

/// A world with a live agent among the ancestors.
pub fn agent_world() -> World {
    let mut world = world();
    world.callers = vec![caller(1, 400)];
    world
        .seen
        .insert(400, PidObservation::Present(identity(400)));
    world
}

pub fn page_error(world: &World) -> ReadError {
    first(world, 10).unwrap_err()
}

pub fn status_error(world: &World) -> ReadError {
    status_of(world, "ab").unwrap_err()
}

pub fn request_of(limit: i64) -> hf_application::ctl::task_reads::PendingPageRequest {
    request(limit, PendingCursorArg::Absent)
}
