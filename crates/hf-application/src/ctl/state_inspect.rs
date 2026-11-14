//! Lease-free classification of a state root from observed facts.
//!
//! Nothing here opens a database, takes a lease or creates anything. `ready` means the
//! identity marker and database header agree with this engine, the `-wal` file is a
//! regular file, the lost-WAL acceptance record, if present, is `accepted` for the
//! marker's generation, no verification fence is present, and `store.watermark` holds a
//! valid slot for that generation. It does not verify database integrity, the
//! generation stored inside the database, or the database sequence against the
//! watermark, so a store that lost acknowledged commits still reads `ready` here (the
//! read session refuses it). A current store without a regular `-wal` is
//! `incompatible`, because every read of it is refused.
use hf_domain::ctl::{
    inspect::{
        EntryKind, FileState, InspectReason, InspectStatus, JournalDecode, MarkerDecode,
        ObservedField, RootFacts, RootStat, StateInspection,
    },
    state_root::StateRoot,
};
mod records;
const LOCK_NAME: &str = ".harflow-store.lock";
const PENDING_NAME: &str = ".harflow-store.json.pending";
const LEGACY_EMPTY: [&str; 4] = [
    "workflows",
    "workflow-runs",
    "workflow-leases",
    "workflow-owners",
];
pub trait StateObserverPort {
    fn observe(&self, root: &StateRoot) -> RootFacts;
}
pub trait StateCodecPort {
    fn decode_marker(&self, bytes: &[u8]) -> MarkerDecode;
    fn decode_journal(&self, bytes: &[u8], root: &StateRoot) -> JournalDecode;
}
/// Identity of the current store layout, supplied by the composition root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorePolicy {
    pub schema_version: u32,
    pub application_id: u32,
}
struct Verdict {
    status: InspectStatus,
    reason: InspectReason,
    observed: Vec<ObservedField>,
}
impl Verdict {
    fn new(status: InspectStatus, reason: InspectReason) -> Self {
        Self {
            status,
            reason,
            observed: Vec::new(),
        }
    }
    fn incompatible(reason: InspectReason) -> Self {
        Self::new(InspectStatus::Incompatible, reason)
    }
    fn interrupted() -> Self {
        Self::new(
            InspectStatus::Interrupted,
            InspectReason::RecoveryInterrupted,
        )
    }
    fn ready() -> Self {
        Self::new(InspectStatus::Ready, InspectReason::Compatible)
    }
    fn observing(mut self, observed: Vec<ObservedField>) -> Self {
        self.observed = observed;
        self
    }
}
/// How a read sees a state root, before any store is opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RootClass {
    /// The root does not exist.
    Absent,
    /// An empty root, or Python's admissible empty set.
    Empty,
    /// A root whose marker and header name a current store.
    MarkedStore,
    /// Any root a read must refuse.
    Refused(StateInspection),
}
/// Classifies a root exactly as `inspect_state` does, so reads and `state-inspect` cannot drift.
pub fn classify_root(
    observer: &dyn StateObserverPort,
    codec: &dyn StateCodecPort,
    policy: StorePolicy,
    root: &StateRoot,
) -> RootClass {
    let facts = observer.observe(root);
    let verdict = classify(&facts, codec, policy, root);
    match verdict.status {
        InspectStatus::Missing => RootClass::Absent,
        InspectStatus::Ready if facts.store_identity == FileState::Absent => RootClass::Empty,
        InspectStatus::Ready => RootClass::MarkedStore,
        _ => RootClass::Refused(StateInspection::new(
            root,
            verdict.status,
            verdict.reason,
            verdict.observed,
        )),
    }
}
pub fn inspect_state(
    observer: &dyn StateObserverPort,
    codec: &dyn StateCodecPort,
    policy: StorePolicy,
    root: &StateRoot,
) -> StateInspection {
    let facts = observer.observe(root);
    let verdict = classify(&facts, codec, policy, root);
    StateInspection::new(root, verdict.status, verdict.reason, verdict.observed)
}
fn classify(
    facts: &RootFacts,
    codec: &dyn StateCodecPort,
    policy: StorePolicy,
    root: &StateRoot,
) -> Verdict {
    match facts.stat {
        RootStat::Missing => {
            return Verdict::new(InspectStatus::Missing, InspectReason::MissingRoot);
        }
        RootStat::Unsafe => return Verdict::incompatible(InspectReason::UnsafeRoot),
        RootStat::Unreadable => {
            return Verdict::incompatible(InspectReason::InfrastructureUnavailable);
        }
        RootStat::Present => {}
    }
    if has_non_regular_infrastructure(facts) {
        return Verdict::incompatible(InspectReason::InvalidMarker);
    }
    let marker = match &facts.python_marker {
        FileState::Bytes(bytes) => Some(codec.decode_marker(bytes)),
        FileState::NotRegular | FileState::Oversize => Some(MarkerDecode::Invalid),
        FileState::Absent | FileState::Unreadable => None,
    };
    if let Some(verdict) = journal_verdict(facts, codec, root, marker.as_ref()) {
        return verdict;
    }
    if facts.python_marker == FileState::Unreadable {
        return Verdict::incompatible(InspectReason::InfrastructureUnavailable);
    }
    match marker {
        Some(MarkerDecode::Invalid) => Verdict::incompatible(InspectReason::InvalidMarker),
        Some(MarkerDecode::Foreign { observed }) => {
            Verdict::incompatible(InspectReason::ForeignStore).observing(observed)
        }
        Some(MarkerDecode::Store { observed, .. }) => {
            Verdict::incompatible(InspectReason::UnsupportedLayout).observing(observed)
        }
        None => store_identity_verdict(facts, policy).unwrap_or_else(|| empty_verdict(facts)),
    }
}
/// The reference checks these names, in this order, before it reads any journal. A
/// directory or symlink lock is `invalid_marker` here (the reference reports
/// `infrastructure_unavailable` only because its message contains the word "lock").
fn has_non_regular_infrastructure(facts: &RootFacts) -> bool {
    let special = |name: &str| {
        facts
            .entries
            .iter()
            .any(|entry| entry.name == name && entry.kind != EntryKind::File)
    };
    special(LOCK_NAME)
        || facts.python_marker == FileState::NotRegular
        || special(PENDING_NAME)
        || facts.journal == FileState::NotRegular
        || facts.journal_pending == FileState::NotRegular
}
/// Mirrors the reference engine. Unreadable or unparseable bytes and an oversize file
/// say nothing about a recovery (`infrastructure_unavailable`); a non-regular file is an
/// invalid marker; a document that is not a journal for this root, and a journal never
/// committed by the marker, are `recovery_interrupted`. Only a well-formed unfinished
/// journal reports status `interrupted`.
fn journal_verdict(
    facts: &RootFacts,
    codec: &dyn StateCodecPort,
    root: &StateRoot,
    marker: Option<&MarkerDecode>,
) -> Option<Verdict> {
    let state = if facts.journal_pending == FileState::Absent {
        &facts.journal
    } else {
        &facts.journal_pending
    };
    let bytes = match state {
        FileState::Absent => return None,
        FileState::Unreadable | FileState::Oversize => {
            return Some(Verdict::incompatible(
                InspectReason::InfrastructureUnavailable,
            ));
        }
        FileState::NotRegular => return Some(Verdict::incompatible(InspectReason::InvalidMarker)),
        FileState::Bytes(bytes) => bytes,
    };
    let (completed, operation_id) = match codec.decode_journal(bytes, root) {
        JournalDecode::Unparseable => {
            return Some(Verdict::incompatible(
                InspectReason::InfrastructureUnavailable,
            ));
        }
        JournalDecode::Unqualified => {
            return Some(Verdict::incompatible(InspectReason::RecoveryInterrupted));
        }
        JournalDecode::Qualified {
            completed,
            operation_id,
        } => (completed, operation_id),
    };
    let committed = matches!(
        marker,
        Some(MarkerDecode::Store { recovery_operation_id: Some(id), .. }) if *id == operation_id
    );
    if !completed && !committed {
        return Some(Verdict::interrupted());
    }
    // The marker gate looks at the main journal file only, after the marker itself reads.
    let marker_readable = facts.python_marker != FileState::Unreadable
        && !matches!(marker, Some(MarkerDecode::Invalid));
    (facts.journal != FileState::Absent && marker_readable && !committed)
        .then(|| Verdict::incompatible(InspectReason::RecoveryInterrupted))
}
fn store_identity_verdict(facts: &RootFacts, policy: StorePolicy) -> Option<Verdict> {
    let bytes = match &facts.store_identity {
        FileState::Absent => return None,
        FileState::Bytes(bytes) => bytes,
        FileState::Unreadable => {
            return Some(Verdict::incompatible(
                InspectReason::InfrastructureUnavailable,
            ));
        }
        FileState::NotRegular | FileState::Oversize => {
            return Some(Verdict::incompatible(InspectReason::InvalidMarker));
        }
    };
    let Some((version, generation)) = parse_identity(bytes) else {
        return Some(Verdict::incompatible(InspectReason::InvalidMarker));
    };
    if version > policy.schema_version {
        return Some(Verdict::incompatible(InspectReason::NewerStore));
    }
    if version < policy.schema_version {
        return Some(Verdict::incompatible(InspectReason::UnsupportedLayout));
    }
    let verdict = header_verdict(&facts.database_header, policy);
    // Only where the verdict would be `ready`: every other row keeps its precedence.
    if verdict.status == InspectStatus::Ready && facts.wal != Some(EntryKind::File) {
        return Some(Verdict::incompatible(
            InspectReason::InfrastructureUnavailable,
        ));
    }
    if verdict.status == InspectStatus::Ready
        && let Some(reason) = records::acceptance_refusal(&facts.wal_acceptance, generation)
            .or_else(|| records::fence_refusal(&facts.fence, generation))
            .or_else(|| records::watermark_refusal(&facts.watermark, generation))
    {
        return Some(Verdict::incompatible(reason));
    }
    Some(verdict)
}

/// Parses `harflow-storage-2\n<schema>\n<32 lowercase hex>\n` exactly.
fn parse_identity(bytes: &[u8]) -> Option<(u32, &str)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut parts = text.split('\n');
    if parts.next()? != "harflow-storage-2" {
        return None;
    }
    let version_text = parts.next()?;
    let version: u32 = version_text.parse().ok()?;
    if version.to_string() != version_text {
        return None;
    }
    let generation = parts.next()?;
    let hex = |byte: u8| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte);
    if generation.len() != 32 || !generation.bytes().all(hex) {
        return None;
    }
    if !parts.next()?.is_empty() || parts.next().is_some() {
        return None;
    }
    Some((version, generation))
}
fn header_verdict(header: &FileState, policy: StorePolicy) -> Verdict {
    let bytes = match header {
        FileState::Bytes(bytes) => bytes,
        FileState::Unreadable => {
            return Verdict::incompatible(InspectReason::InfrastructureUnavailable);
        }
        _ => return Verdict::incompatible(InspectReason::InvalidMarker),
    };
    if bytes.len() < 100 || bytes.get(..16) != Some(b"SQLite format 3\0".as_slice()) {
        return Verdict::incompatible(InspectReason::InvalidMarker);
    }
    let word =
        |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    if word(68) != policy.application_id {
        return Verdict::incompatible(InspectReason::ForeignStore);
    }
    let version = word(60);
    if version > policy.schema_version {
        return Verdict::incompatible(InspectReason::NewerStore);
    }
    if version != policy.schema_version {
        return Verdict::incompatible(InspectReason::InvalidMarker);
    }
    Verdict::ready()
}
fn empty_verdict(facts: &RootFacts) -> Verdict {
    if facts.entries_truncated {
        return Verdict::incompatible(InspectReason::UnmarkedNonempty);
    }
    let mut rest = Vec::new();
    for entry in &facts.entries {
        if entry.name == LOCK_NAME || entry.name == PENDING_NAME {
            if entry.kind != EntryKind::File {
                return Verdict::incompatible(InspectReason::InvalidMarker);
            }
        } else {
            rest.push(entry);
        }
    }
    let legacy = rest.len() == LEGACY_EMPTY.len()
        && rest.iter().all(|entry| {
            LEGACY_EMPTY.contains(&entry.name.as_str())
                && entry.kind == EntryKind::Directory
                && entry.empty_directory
        });
    if rest.is_empty() || legacy {
        Verdict::ready()
    } else {
        Verdict::incompatible(InspectReason::UnmarkedNonempty)
    }
}
