//! The continuation-stop observation of a task: whether the chain it belongs to was stopped.
//! Pure projection of a task's stored chain root and the stored stop row of that root.
use crate::{ctl::task_rows::ChainRoot, task_read::TaskId};

/// What the chain-stop table holds for one chain root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopRow {
    /// No row: the chain is not stopped.
    Absent,
    /// A marker. `attribution` is the stored attribution text when it is at most 64 bytes
    /// long and not empty; the projection keeps it only if it is a task identity.
    Marker { attribution: Option<String> },
    /// The stop state could not be established.
    Fault,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopState {
    Stopped,
    NotStopped,
    Unknown,
}

impl StopState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::NotStopped => "not_stopped",
            Self::Unknown => "unknown",
        }
    }
}

/// The observation, without its reader-clock stamp.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContinuationStop {
    pub state: StopState,
    /// The chain root observed: the stored root, or the task itself when none is stored.
    /// `None` when the stored root is not a task identity.
    pub root: Option<TaskId>,
    pub cancelled: Option<TaskId>,
}

impl ContinuationStop {
    /// An absent chain root means the task is its own root. A stored root that is not a task
    /// identity (the empty string included) is `unknown` with no root, whatever the row says.
    pub fn observe(task: &TaskId, chain_root: &ChainRoot, row: &StopRow) -> Self {
        let root = match chain_root {
            ChainRoot::Absent => task.clone(),
            ChainRoot::Valid(root) => root.clone(),
            ChainRoot::Invalid(_) => {
                return Self {
                    state: StopState::Unknown,
                    root: None,
                    cancelled: None,
                };
            }
        };
        let (state, cancelled) = match row {
            StopRow::Absent => (StopState::NotStopped, None),
            StopRow::Fault => (StopState::Unknown, None),
            StopRow::Marker { attribution } => (
                StopState::Stopped,
                attribution.clone().and_then(|text| TaskId::new(text).ok()),
            ),
        };
        Self {
            state,
            root: Some(root),
            cancelled,
        }
    }
}
