//! The caller gate: root classification, markers, the process view and detection.
use super::{ReadError, Reads, StoreFailure, TaskSnapshotPort, TaskView, ViewFailure};
use crate::ctl::state_inspect::RootClass;
use hf_domain::ctl::{
    caller::{
        CallerClass, GatePath, IdentityVerdict, MISSING_ROOT_READS_EMPTY, PidObservation,
        ProcessView, RefusalKind, admit, classify, marker_refusal, verdict,
    },
    inspect::{InspectReason, InspectStatus, StateInspection},
    state_root::StateRoot,
    task_projection::{is_unobserved, project},
    task_rows::{ProcessRecord, TaskRecord},
};
use std::{collections::HashMap, time::Instant};

/// What the root and the environment allow before any store is opened.
pub(crate) enum Entry {
    /// No store: answer from the root alone, creating nothing.
    NoStore,
    Store {
        marker: Option<RefusalKind>,
        /// `None` when a marker made the process view unnecessary.
        view: Option<Result<ProcessView, ViewFailure>>,
    },
}

/// Classifies the root, applies the marker rule of `path` and takes the process view.
/// Only the plain page read defers a marker refusal until after the damage check.
pub(crate) fn enter(
    reads: &Reads<'_>,
    root: &StateRoot,
    path: GatePath,
) -> Result<Entry, ReadError> {
    enter_checked(reads, root, path, &|| Ok(()))
}

/// As `enter`, running `check` (request syntax) after the root is qualified and before any
/// marker is considered, which is the reference's order.
pub(crate) fn enter_checked(
    reads: &Reads<'_>,
    root: &StateRoot,
    path: GatePath,
    check: &dyn Fn() -> Result<(), ReadError>,
) -> Result<Entry, ReadError> {
    let class = reads.roots.classify(root);
    if let RootClass::Refused(inspection) = class {
        return Err(ReadError::State(inspection));
    }
    if class == RootClass::Absent && !MISSING_ROOT_READS_EMPTY {
        return Err(ReadError::State(StateInspection::new(
            root,
            InspectStatus::Missing,
            InspectReason::MissingRoot,
            Vec::new(),
        )));
    }
    check()?;
    let marker = marker_refusal(path, reads.markers);
    if class != RootClass::MarkedStore {
        return match marker {
            Some(kind) => Err(ReadError::Refused(kind)),
            None => Ok(Entry::NoStore),
        };
    }
    if let (Some(kind), false) = (marker, path == GatePath::Page) {
        return Err(ReadError::Refused(kind));
    }
    let view = marker.is_none().then(|| reads.process.view(reads.deadline));
    Ok(Entry::Store { marker, view })
}

pub(crate) fn timely(deadline: Instant) -> Result<(), ReadError> {
    if Instant::now() >= deadline {
        Err(ReadError::Store(StoreFailure::Busy))
    } else {
        Ok(())
    }
}

/// Raw observations of pids, taken at most once per command.
pub(crate) struct Observer<'a> {
    port: &'a dyn super::ProcessViewPort,
    seen: HashMap<u32, PidObservation>,
}

impl<'a> Observer<'a> {
    pub(crate) fn new(port: &'a dyn super::ProcessViewPort) -> Self {
        Self {
            port,
            seen: HashMap::new(),
        }
    }

    /// Each record is compared against the shared observation separately, because several
    /// records can share a pid after reuse.
    pub(crate) fn verdict(&mut self, process: Option<&ProcessRecord>) -> IdentityVerdict {
        let Some(process) = process else {
            return IdentityVerdict::Undecidable;
        };
        let port = self.port;
        let observed = self
            .seen
            .entry(process.pid)
            .or_insert_with(|| port.observe(process.pid));
        verdict(process.start.as_ref(), observed)
    }

    /// The task with its live projection.
    pub(crate) fn view(&mut self, record: TaskRecord) -> TaskView {
        let facts = &record.facts;
        let live = if is_unobserved(&facts.status, facts.exit_code) {
            self.verdict(record.process.as_ref())
        } else {
            IdentityVerdict::Dead
        };
        let projection = project(&facts.status, facts.exit_code, live);
        TaskView { record, projection }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Family {
    /// The catalog family: the first 100 ancestors and at most 100 related rows.
    Bounded,
    /// Every ancestor and every related row.
    Complete,
}

const BOUNDED: usize = 100;

fn detect(
    snapshot: &dyn TaskSnapshotPort,
    view: &Result<ProcessView, ViewFailure>,
    family: Family,
    observer: &mut Observer<'_>,
) -> Result<CallerClass, ReadError> {
    let Ok(view) = view else {
        return Ok(CallerClass::Undecidable);
    };
    let (ancestors, limit) = match family {
        Family::Bounded => (
            &view.ancestors[..view.ancestors.len().min(BOUNDED)],
            Some(BOUNDED),
        ),
        Family::Complete => (&view.ancestors[..], None),
    };
    let rows = snapshot.related_callers(view.session, ancestors, limit)?;
    if limit.is_some_and(|cap| rows.len() > cap) {
        return Ok(CallerClass::Undecidable);
    }
    let verdicts: Vec<IdentityVerdict> = rows
        .iter()
        .map(|row| observer.verdict(Some(&row.process)))
        .collect();
    Ok(classify(&verdicts))
}

/// Refuses on a marker, then on the detected class. Runs inside the snapshot, after the
/// damage check, before anything about any task is read.
pub(crate) fn authorize(
    path: GatePath,
    snapshot: &dyn TaskSnapshotPort,
    marker: Option<RefusalKind>,
    view: &Option<Result<ProcessView, ViewFailure>>,
    family: Family,
    observer: &mut Observer<'_>,
) -> Result<(), ReadError> {
    if let Some(kind) = marker {
        return Err(ReadError::Refused(kind));
    }
    let class = match view {
        Some(view) => detect(snapshot, view, family, observer)?,
        None => CallerClass::Undecidable,
    };
    match admit(path, class) {
        Some(kind) => Err(ReadError::Refused(kind)),
        None => Ok(()),
    }
}

pub(crate) fn check_damage(snapshot: &dyn TaskSnapshotPort) -> Result<(), ReadError> {
    if snapshot.damaged()? {
        Err(ReadError::Damaged)
    } else {
        Ok(())
    }
}

/// Damage (or incomplete coverage) first, then the gate.
pub(crate) fn guard(
    path: GatePath,
    snapshot: &dyn TaskSnapshotPort,
    marker: Option<RefusalKind>,
    view: &Option<Result<ProcessView, ViewFailure>>,
    family: Family,
    observer: &mut Observer<'_>,
) -> Result<(), ReadError> {
    check_damage(snapshot)?;
    authorize(path, snapshot, marker, view, family, observer)
}
