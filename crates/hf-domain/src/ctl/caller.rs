//! Caller classification for authority-gated reads, as pure policy.
//!
//! The admitted class is `NoAgentDetected`: no agent evidence was found after a complete
//! look. It is the absence of evidence, not a credential, and it is never consulted for a
//! mutation. The two named switches below are the only places the owner decisions on
//! unresolved agents and missing roots are encoded.
use crate::process::ProcessIdentity;

/// Agent callers whose workflow ownership cannot be read are refused on the managed reads
/// (`Managed` and `Page` paths) until workflow ownership is readable. Direct-only reads
/// (`Successors`) refuse an agent whatever this says. Turning it off also needs the
/// environment-id detection method, which is not built: a marker alone cannot be verified.
pub const REFUSE_UNRESOLVED_AGENTS: bool = true;
/// A read of a root that holds no store answers empty and creates nothing.
pub const MISSING_ROOT_READS_EMPTY: bool = true;

/// Environment variables that mark a caller as an agent.
pub const MARKER_KEYS: [&str; 4] = ["HARFLOW_TASK_ID", "PB_TASK_ID", "HARFLOW_DEPTH", "PB_DEPTH"];
/// A recorded task is observed-terminal when its status is one of these and it has an exit code.
pub const TERMINAL_STATUSES: [&str; 4] = ["completed", "failed", "timed_out", "cancelled"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallerClass {
    NoAgentDetected,
    Agent,
    Undecidable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityVerdict {
    Alive,
    Dead,
    Undecidable,
}

/// Why a read is refused; the transport chooses the wire code for its path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalKind {
    Agent,
    Undecidable,
}

/// Which gate family a path belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatePath {
    /// `status`, `status-batch` and `list`: a marker counts when it is non-empty.
    Managed,
    /// The plain page read: a marker counts when it is non-empty.
    Page,
    /// Successor discovery: any present marker counts, empty values included.
    Successors,
}

/// What the environment says about the four marker variables.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnvMarkers {
    pub present: bool,
    pub non_empty: bool,
}

impl EnvMarkers {
    /// `lookup` returns `None` for an unset variable and whether the value is non-empty otherwise.
    pub fn observe(lookup: &dyn Fn(&str) -> Option<bool>) -> Self {
        let mut markers = Self::default();
        for key in MARKER_KEYS {
            if let Some(non_empty) = lookup(key) {
                markers.present = true;
                markers.non_empty |= non_empty;
            }
        }
        markers
    }
}

/// Whether an agent is refused on `path` when unresolved agents are (`refuse_unresolved`) or
/// are not refused on the managed reads. A direct-only read refuses an agent either way.
pub fn agent_refused(path: GatePath, refuse_unresolved: bool) -> bool {
    path == GatePath::Successors || refuse_unresolved
}

/// The refusal an environment marker alone causes, before any lineage is read. A marker on a
/// direct-only path is always an agent.
pub fn marker_refusal(path: GatePath, markers: EnvMarkers) -> Option<RefusalKind> {
    let counts = match path {
        GatePath::Successors => markers.present,
        GatePath::Managed | GatePath::Page => markers.non_empty,
    };
    if !counts {
        return None;
    }
    if agent_refused(path, REFUSE_UNRESOLVED_AGENTS) {
        Some(RefusalKind::Agent)
    } else {
        // The marker names a task this engine cannot verify without env-id detection.
        Some(RefusalKind::Undecidable)
    }
}

/// The refusal a detected class causes on `path`, if any.
pub fn admit(path: GatePath, class: CallerClass) -> Option<RefusalKind> {
    match class {
        CallerClass::NoAgentDetected => None,
        CallerClass::Undecidable => Some(RefusalKind::Undecidable),
        CallerClass::Agent if agent_refused(path, REFUSE_UNRESOLVED_AGENTS) => {
            Some(RefusalKind::Agent)
        }
        CallerClass::Agent => None,
    }
}

/// Whether a recorded task can still be a caller: not observed-terminal. The storage query
/// builds its predicate from `TERMINAL_STATUSES`, the same source.
pub fn is_candidate(status: &str, exit_code: Option<i64>) -> bool {
    !(TERMINAL_STATUSES.contains(&status) && exit_code.is_some())
}

/// What one pid looked like when observed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PidObservation {
    Present(ProcessIdentity),
    Zombie,
    Absent,
    Unknown,
}

/// The verdict for one recorded process against one observation. Missing identity columns,
/// an unreadable pid and a zombie are undecidable.
pub fn verdict(recorded: Option<&ProcessIdentity>, observed: &PidObservation) -> IdentityVerdict {
    let Some(recorded) = recorded else {
        return IdentityVerdict::Undecidable;
    };
    match observed {
        PidObservation::Absent => IdentityVerdict::Dead,
        PidObservation::Present(actual) if actual == recorded => IdentityVerdict::Alive,
        PidObservation::Present(_) => IdentityVerdict::Dead,
        PidObservation::Zombie | PidObservation::Unknown => IdentityVerdict::Undecidable,
    }
}

/// Any alive related candidate is an agent; otherwise any undecidable one makes the whole
/// detection undecidable.
pub fn classify(verdicts: &[IdentityVerdict]) -> CallerClass {
    if verdicts.contains(&IdentityVerdict::Alive) {
        CallerClass::Agent
    } else if verdicts.contains(&IdentityVerdict::Undecidable) {
        CallerClass::Undecidable
    } else {
        CallerClass::NoAgentDetected
    }
}

/// The process table facts of the calling process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessView {
    pub pid: u32,
    pub session: i64,
    /// Nearest first, excluding the process itself.
    pub ancestors: Vec<u32>,
}

/// Walks the parent chain of `pid` over `(pid, parent)` pairs. The chain excludes `pid` and
/// stops at parent 0 or 1, at a missing entry and at a cycle; the stopping pid is not
/// included. `None` when `pid` itself is missing from the table.
pub fn ancestors_of(pid: u32, table: &[(u32, u32)]) -> Option<Vec<u32>> {
    let parent_of = |wanted: u32| table.iter().find(|row| row.0 == wanted).map(|row| row.1);
    parent_of(pid)?;
    let mut chain: Vec<u32> = Vec::new();
    let mut current = pid;
    while let Some(parent) = parent_of(current) {
        if parent <= 1 || parent == pid || chain.contains(&parent) {
            break;
        }
        chain.push(parent);
        current = parent;
    }
    Some(chain)
}
