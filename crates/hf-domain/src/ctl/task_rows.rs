//! Plain rows the task reads exchange between the store and the use cases.
use crate::{process::ProcessIdentity, task_read::TaskId};

/// The recorded process of a task. `start` is `None` when the identity cannot be decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessRecord {
    pub pid: u32,
    pub start: Option<ProcessIdentity>,
}

/// A recorded task that may be the caller: its status, exit code and process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallerRow {
    pub task: TaskId,
    pub status: String,
    pub exit_code: Option<i64>,
    pub process: ProcessRecord,
}

/// The typed facts of one valid task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskFacts {
    pub id: TaskId,
    pub backend: String,
    pub repo_path: String,
    pub session_id: String,
    pub status: String,
    pub exit_code: Option<i64>,
    pub started_at: String,
    pub started_at_us: i64,
    pub finished_at: Option<String>,
    pub finished_at_us: Option<i64>,
    pub output_drained: bool,
    pub title: Option<String>,
    pub model: Option<String>,
    pub continuation_of: Option<TaskId>,
    pub chain_root: Option<TaskId>,
}

/// One task as stored: facts, the served header and status bytes, and its process row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRecord {
    pub facts: TaskFacts,
    pub header: Vec<u8>,
    pub status_blob: Option<Vec<u8>>,
    pub process: Option<ProcessRecord>,
}

/// A task named by an ambiguous prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskSummary {
    pub id: TaskId,
    pub title: Option<String>,
    pub status: String,
    pub created_at: String,
}

/// One keyset batch of tasks, newest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BatchQuery {
    pub after: Option<(i64, String)>,
    pub session: Option<String>,
    pub predecessor: Option<TaskId>,
    pub since_us: Option<i64>,
    pub unobserved_only: bool,
    pub max_rows: usize,
}

/// A decoded wire cursor: the scope digest and the last row returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireCursor {
    pub scope: String,
    pub stamp: i64,
    pub id: String,
}

/// An unobserved row with only what counting needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnobservedRow {
    pub id: TaskId,
    pub stamp: i64,
    pub session: String,
    pub status: String,
    pub exit_code: Option<i64>,
    pub process: Option<ProcessRecord>,
}
