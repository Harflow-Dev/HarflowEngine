//! Values for the lease-free state observer: results and observed facts.
use super::state_root::StateRoot;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectStatus {
    Ready,
    Missing,
    Incompatible,
    Interrupted,
}
impl InspectStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Missing => "missing",
            Self::Incompatible => "incompatible",
            Self::Interrupted => "interrupted",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectReason {
    Compatible,
    MissingRoot,
    UnmarkedNonempty,
    ForeignStore,
    InvalidMarker,
    UnsupportedLayout,
    NewerStore,
    RecoveryInterrupted,
    UnsafeRoot,
    InfrastructureUnavailable,
}
impl InspectReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::MissingRoot => "missing_root",
            Self::UnmarkedNonempty => "unmarked_nonempty",
            Self::ForeignStore => "foreign_store",
            Self::InvalidMarker => "invalid_marker",
            Self::UnsupportedLayout => "unsupported_layout",
            Self::NewerStore => "newer_store",
            Self::RecoveryInterrupted => "recovery_interrupted",
            Self::UnsafeRoot => "unsafe_root",
            Self::InfrastructureUnavailable => "infrastructure_unavailable",
        }
    }
    /// The reference engine's message family: refusals wrap a short detail in the
    /// standard sentence; an unfinished recovery has its own text.
    pub fn message(self, status: InspectStatus) -> String {
        let detail = match self {
            Self::Compatible => return "State is compatible".to_string(),
            Self::MissingRoot => return "State root has not been initialized".to_string(),
            Self::UnmarkedNonempty => return "Unmarked state root contains data".to_string(),
            Self::UnsafeRoot => {
                return "Recovery root and its parents must be directories without symlinks"
                    .to_string();
            }
            Self::InfrastructureUnavailable => return "State root cannot be read".to_string(),
            Self::RecoveryInterrupted if status == InspectStatus::Interrupted => {
                return "Explicit recovery must be resumed".to_string();
            }
            Self::RecoveryInterrupted => "explicit state recovery is interrupted",
            Self::ForeignStore => "foreign store marker",
            Self::InvalidMarker => "invalid store marker",
            Self::UnsupportedLayout => "unsupported store layout",
            Self::NewerStore => "incompatible store product line",
        };
        let mut text = String::from("Unsupported state root: ");
        text.push_str(detail);
        text.push_str("; inspect state or select a fresh state root");
        text
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scalar {
    Int(i64),
    Text(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedField {
    pub key: &'static str,
    pub value: Scalar,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateInspection {
    pub state_root: String,
    pub status: InspectStatus,
    pub reason: InspectReason,
    pub observed_store: Vec<ObservedField>,
}
impl StateInspection {
    pub fn new(
        root: &StateRoot,
        status: InspectStatus,
        reason: InspectReason,
        observed_store: Vec<ObservedField>,
    ) -> Self {
        Self {
            state_root: root.as_str().to_string(),
            status,
            reason,
            observed_store,
        }
    }
}
/// Result of reading one bounded file without following links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileState {
    Absent,
    NotRegular,
    Oversize,
    Unreadable,
    Bytes(Vec<u8>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Other,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootEntry {
    pub name: String,
    pub kind: EntryKind,
    pub empty_directory: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootStat {
    Missing,
    Unsafe,
    Unreadable,
    Present,
}
/// Everything the classifier may use. Gathering it creates and writes nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootFacts {
    pub stat: RootStat,
    pub entries: Vec<RootEntry>,
    pub entries_truncated: bool,
    pub python_marker: FileState,
    pub store_identity: FileState,
    pub database_header: FileState,
    pub journal: FileState,
    pub journal_pending: FileState,
}
impl RootFacts {
    pub fn without_files(stat: RootStat) -> Self {
        Self {
            stat,
            entries: Vec::new(),
            entries_truncated: false,
            python_marker: FileState::Absent,
            store_identity: FileState::Absent,
            database_header: FileState::Absent,
            journal: FileState::Absent,
            journal_pending: FileState::Absent,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkerDecode {
    Invalid,
    Foreign {
        observed: Vec<ObservedField>,
    },
    Store {
        observed: Vec<ObservedField>,
        recovery_operation_id: Option<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JournalDecode {
    /// Not a JSON document at all.
    Unparseable,
    /// A document that is not a journal for this root.
    Unqualified,
    Qualified {
        completed: bool,
        operation_id: String,
    },
}
