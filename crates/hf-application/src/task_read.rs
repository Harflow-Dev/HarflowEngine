//! Constructor-injected internal task reads. Authority precedes every repository call.
use hf_domain::task_read::{
    AuthorizedScope, Coverage, Cursor, Lookup, QueryError, StoredObservation, TaskId, TaskQuery,
};
use std::time::Instant;
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Authority {
    Preparing,
    Blocked,
    Authorized(AuthorizedScope),
}
pub trait TaskAuthorityPort {
    fn prepare(&self) -> Authority;
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskReadError {
    Preparing,
    Refused,
    Unavailable,
    InvalidQuery(QueryError),
    Storage,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskRow {
    pub id: TaskId,
    pub stamp: i64,
    pub session: Option<String>,
    pub observation: StoredObservation,
    pub header: Vec<u8>,
    pub status: Option<Vec<u8>>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskReadResult {
    pub rows: Vec<TaskRow>,
    pub lookups: Vec<Lookup>,
    pub cursor: Option<Cursor>,
    /// Count of matching stored rows, unknown when evidence is incomplete.
    pub count: Option<usize>,
    /// Activity evidence is known for the query, including skipped active-only candidates.
    pub activity_complete: bool,
    pub coverage: Coverage,
    /// More indexed rows remain; independent of evidence completeness.
    pub partial: bool,
}
pub trait TaskReadPort {
    fn execute(
        &self,
        scope: &AuthorizedScope,
        query: &TaskQuery,
        deadline: Instant,
    ) -> Result<TaskReadResult, TaskReadError>;
}
pub struct TaskReader<A, R> {
    authority: A,
    repository: R,
}
impl<A: TaskAuthorityPort, R: TaskReadPort> TaskReader<A, R> {
    pub fn new(authority: A, repository: R) -> Self {
        Self {
            authority,
            repository,
        }
    }
    pub fn read(
        &self,
        query: &TaskQuery,
        deadline: Instant,
    ) -> Result<TaskReadResult, TaskReadError> {
        let scope = match self.authority.prepare() {
            Authority::Preparing => return Err(TaskReadError::Preparing),
            Authority::Blocked => return Err(TaskReadError::Refused),
            Authority::Authorized(scope) => scope,
        };
        scope.validate().map_err(|_| TaskReadError::Refused)?;
        query.validate().map_err(TaskReadError::InvalidQuery)?;
        self.repository.execute(&scope, query, deadline)
    }
}
