//! Event read result values.
use hf_domain::{
    event_read::{EventCursor, EventGap, LogFacts},
    task_read::{Lookup, TaskId},
};
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
