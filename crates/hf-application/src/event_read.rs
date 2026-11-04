//! Injected fixture-only event authority. No production caller verification.
use hf_domain::{
    event_read::{EventCursor, EventError, EventGap, EventQuery, LogFacts},
    task_read::{Lookup, TaskId},
};
use std::time::Instant;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventAuthority {
    Preparing,
    Blocked,
    Managed,
    DirectFixture,
}
pub trait EventAuthorityPort {
    fn prepare(&self) -> EventAuthority;
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventReadError {
    Preparing,
    Refused,
    Unavailable,
    InvalidQuery(EventError),
    Storage,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordClass {
    Valid,
    Blank,
    Malformed,
    Oversized,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventRecord {
    pub start: u64,
    pub end: u64,
    pub class: RecordClass,
    pub seq: Option<u64>,
    pub kind: Option<String>,
    pub body: Vec<u8>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventReadResult {
    pub task: Option<TaskId>,
    pub lookup: Lookup,
    pub events: Vec<EventRecord>,
    pub older_cursor: Option<EventCursor>,
    pub newer_cursor: Option<EventCursor>,
    pub has_older: bool,
    pub has_newer: bool,
    pub span: Option<(u64, u64)>,
    pub malformed: usize,
    pub oversized: usize,
    pub gaps: Vec<EventGap>,
    pub facts: Option<LogFacts>,
    pub decoded: usize,
    /// Logical bytes inspected, at most one 1MiB window. Not serialized size.
    pub scanned_bytes: u64,
}
pub trait EventReadPort {
    fn execute(
        &self,
        query: &EventQuery,
        deadline: Instant,
    ) -> Result<EventReadResult, EventReadError>;
}
pub struct EventReader<A, R> {
    authority: A,
    repository: R,
}
impl<A: EventAuthorityPort, R: EventReadPort> EventReader<A, R> {
    pub fn new(authority: A, repository: R) -> Self {
        Self {
            authority,
            repository,
        }
    }
    pub fn read(
        &self,
        query: &EventQuery,
        deadline: Instant,
    ) -> Result<EventReadResult, EventReadError> {
        query.validate().map_err(EventReadError::InvalidQuery)?;
        match self.authority.prepare() {
            EventAuthority::Preparing => return Err(EventReadError::Preparing),
            EventAuthority::Blocked | EventAuthority::Managed => {
                return Err(EventReadError::Refused);
            }
            EventAuthority::DirectFixture => {}
        }
        self.repository.execute(query, deadline)
    }
}
