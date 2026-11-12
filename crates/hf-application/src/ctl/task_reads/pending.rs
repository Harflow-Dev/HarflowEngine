//! `task-pending-page` and `task-pending-status`: bounded discovery of launches that have no
//! record yet, for a verified direct caller only.
//!
//! The order is fixed here: environment markers, root availability (the root is never
//! initialized), the direct-only caller gate, then limit, cursor or task id, then the
//! namespace. Every refusal of this family reports unknown coverage.
use super::{
    PendingBudget, PendingLimits, ReadError, Reads, StoreFailure, TaskSnapshotPort, ViewFailure,
    gate::{Family, Observer, authorize},
    resolve::valid_id,
};
use crate::ctl::state_inspect::RootClass;
use hf_domain::ctl::{
    caller::{GatePath, ProcessView, RefusalKind, marker_refusal},
    pending::{LAUNCH_READ_CHARGE, LaunchFacts, LaunchOutcome, outcome},
    state_root::StateRoot,
};
use hf_domain::task_read::TaskId;
use std::{cell::Cell, time::Instant};

/// Why a pending read ended without an answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingFailure {
    /// An environment marker refused before the root was opened.
    Marker,
    /// The selected state cannot be read as a store.
    Unavailable,
    Check {
        code: &'static str,
        check: &'static str,
    },
}

pub(super) fn failure(code: &'static str, check: &'static str) -> ReadError {
    ReadError::Pending(PendingFailure::Check { code, check })
}

type View = Result<ProcessView, ViewFailure>;

/// Markers, then the root, then the process view. Nothing is opened before both pass.
fn enter(reads: &Reads<'_>, root: &StateRoot) -> Result<View, ReadError> {
    if marker_refusal(GatePath::DirectOnly, reads.markers).is_some() {
        return Err(ReadError::Pending(PendingFailure::Marker));
    }
    if reads.roots.classify(root) != RootClass::MarkedStore {
        return Err(ReadError::Pending(PendingFailure::Unavailable));
    }
    Ok(reads.process.view(reads.deadline))
}

/// The direct-only gate: every ordinary record is checked first, so a record that cannot be
/// qualified, or an exhausted budget, never reads as proof of a person.
fn direct_gate(
    snapshot: &dyn TaskSnapshotPort,
    view: &View,
    (budget, limits): (&PendingBudget, &PendingLimits),
    observer: &mut Observer<'_>,
) -> Result<(), ReadError> {
    let undecidable = ReadError::Refused(RefusalKind::Undecidable);
    // A failure while the caller is being established is not proof of a person.
    if !snapshot
        .candidates_checked(budget, limits)
        .map_err(|_| undecidable.clone())?
    {
        return Err(undecidable);
    }
    let gate = Some(view.clone());
    authorize(
        GatePath::DirectOnly,
        snapshot,
        None,
        &gate,
        Family::Complete,
        observer,
    )
    .map_err(|error| match error {
        ReadError::Store(_) => undecidable,
        other => other,
    })
}

/// A store failure after the gate: the deadline is the read budget, anything else an
/// unavailable source (or namespace, for `check`).
pub(super) fn unavailable(
    check: &'static str,
    deadline: Instant,
) -> impl Fn(StoreFailure) -> ReadError {
    move |failure| match failure {
        StoreFailure::Busy if Instant::now() >= deadline => {
            self::failure("pending_observation_bound", "read_budget")
        }
        _ => self::failure("pending_observation_unavailable", check),
    }
}

/// Refusals and store failures in the vocabulary of this family.
fn pending_error(error: ReadError, deadline: Instant) -> ReadError {
    match error {
        ReadError::Refused(RefusalKind::Agent) => failure("agent_caller", "direct_caller_required"),
        ReadError::Refused(RefusalKind::Undecidable) => {
            failure("caller_undecidable", "direct_caller_required")
        }
        ReadError::Store(StoreFailure::Busy) if Instant::now() >= deadline => {
            failure("pending_observation_bound", "read_budget")
        }
        ReadError::Store(_) => ReadError::Pending(PendingFailure::Unavailable),
        other => other,
    }
}

/// Runs `action` inside the gated snapshot of a pending read.
pub(super) fn gated<T>(
    reads: &Reads<'_>,
    root: &StateRoot,
    limits: &PendingLimits,
    action: &mut dyn FnMut(&dyn TaskSnapshotPort, &PendingBudget) -> Result<T, ReadError>,
) -> Result<T, ReadError> {
    let view = enter(reads, root)?;
    let budget = PendingBudget::new(limits.read_bytes);
    let mut observer = Observer::new(reads.process);
    let mut value = None;
    let undecided = Cell::new(false);
    reads
        .store
        .with_snapshot(root, reads.deadline, &mut |snapshot| {
            if let Err(error) = direct_gate(snapshot, &view, (&budget, limits), &mut observer) {
                undecided.set(error == ReadError::Refused(RefusalKind::Undecidable));
                return Err(error);
            }
            value = Some(action(snapshot, &budget)?);
            Ok(())
        })
        .map_err(|error| match error {
            // The session's exit check fails on a passed deadline alone, after the caller
            // was already found undecidable (for example because the check ran out of
            // time): the verdict stands, as in the reference. Any other exit failure, a
            // replaced store included, outranks it.
            ReadError::Store(StoreFailure::Busy)
                if undecided.get() && Instant::now() >= reads.deadline =>
            {
                pending_error(ReadError::Refused(RefusalKind::Undecidable), reads.deadline)
            }
            other => pending_error(other, reads.deadline),
        })?;
    value.ok_or(ReadError::Internal)
}

/// What reading one candidate found.
pub(super) enum Seen {
    /// An ordinary record has the id; nothing of any receipt was decoded.
    Recorded,
    /// No receipt has the id.
    Missing,
    Launch(Box<LaunchFacts>),
    /// The read budget or the deadline cannot cover the read; nothing was fetched.
    OutOfBudget,
}

/// Reads one candidate, paying first: a record wins at the cost of its stored size, and a
/// receipt is fetched and decoded only after `LAUNCH_READ_CHARGE` is reserved. A receipt
/// larger than the bound comes back refused as `oversize`, undecoded. `known` says the
/// receipt is already known to exist, so its existence is not asked again.
pub(super) fn read_candidate(
    snapshot: &dyn TaskSnapshotPort,
    id: &TaskId,
    (budget, deadline): (&PendingBudget, Instant),
    known: bool,
) -> Result<Seen, ReadError> {
    let source = unavailable("source_unavailable", deadline);
    let expired = || Instant::now() >= deadline;
    if let Some(size) = snapshot.record_size(id).map_err(&source)? {
        return Ok(if expired() || !budget.charge(size) {
            Seen::OutOfBudget
        } else {
            Seen::Recorded
        });
    }
    if !known && !snapshot.launch_name_exists(id.as_str()).map_err(&source)? {
        return Ok(Seen::Missing);
    }
    if expired() || !budget.charge(LAUNCH_READ_CHARGE) {
        return Ok(Seen::OutOfBudget);
    }
    Ok(snapshot
        .pending_row(id)
        .map_err(&source)?
        .map_or(Seen::Missing, |facts| Seen::Launch(Box::new(facts))))
}

/// The answer of `task-pending-status`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingStatusOut {
    pub id: String,
    /// `None`: no record and no receipt has the id.
    pub outcome: Option<LaunchOutcome>,
}

/// One launch by its exact full id; a prefix is never resolved.
pub fn pending_status(
    reads: &Reads<'_>,
    root: &StateRoot,
    raw: &str,
    limits: &PendingLimits,
) -> Result<PendingStatusOut, ReadError> {
    gated(reads, root, limits, &mut |snapshot, budget| {
        let id = valid_id(raw).map_err(|_| failure("invalid_task_id", "exact_task_id_required"))?;
        let outcome = match read_candidate(snapshot, &id, (budget, reads.deadline), false)? {
            Seen::OutOfBudget => return Err(failure("pending_observation_bound", "read_budget")),
            Seen::Missing => None,
            Seen::Recorded => Some(LaunchOutcome::Recorded),
            Seen::Launch(facts) => Some(outcome(&facts)),
        };
        Ok(PendingStatusOut {
            id: id.as_str().to_owned(),
            outcome,
        })
    })
}
