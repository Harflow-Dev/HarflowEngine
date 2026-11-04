//! Internal event paging values; no public cursor encoding or live authority.
use crate::task_read::TaskId;
pub const EVENT_KINDS: [&str; 10] = [
    "task_started",
    "assistant_text",
    "assistant_delta",
    "tool_call",
    "tool_result",
    "user_message",
    "usage",
    "notice",
    "task_finished",
    "undelivered",
];
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventError {
    InvalidLimit,
    InvalidOptions,
    InvalidFilter,
    InvalidCursor,
    ExpiredCursor,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventFilter {
    Default,
    All,
    Kinds(Vec<String>),
}
impl EventFilter {
    pub fn kinds(&self) -> Vec<String> {
        let mut kinds: Vec<String> = match self {
            Self::Default => EVENT_KINDS
                .iter()
                .filter(|kind| **kind != "assistant_delta")
                .map(|kind| String::from(*kind))
                .collect(),
            Self::All => EVENT_KINDS.iter().map(|kind| String::from(*kind)).collect(),
            Self::Kinds(kinds) => kinds.clone(),
        };
        kinds.sort();
        kinds.dedup();
        kinds
    }
    pub fn validate(&self) -> Result<(), EventError> {
        if let Self::Kinds(kinds) = self
            && (kinds.is_empty()
                || kinds.len() > 10
                || kinds
                    .iter()
                    .any(|kind| !EVENT_KINDS.contains(&kind.as_str())))
        {
            return Err(EventError::InvalidFilter);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Older,
    Newer,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventCursor {
    pub store: String,
    pub generation: String,
    pub task: TaskId,
    pub kinds: Vec<String>,
    pub direction: Direction,
    pub boundary: u64,
    pub resync: bool,
    pub neighbour_seq: Option<u64>,
    pub neighbour_skip: bool,
}
impl EventCursor {
    pub fn validate(&self) -> Result<(), EventError> {
        if self.store.len() != 32
            || self.generation.len() != 32
            || !self.store.bytes().all(|b| b.is_ascii_hexdigit())
            || !self.generation.bytes().all(|b| b.is_ascii_hexdigit())
            || self.boundary > i64::MAX as u64
            || self.neighbour_seq.is_some_and(|seq| seq > i64::MAX as u64)
        {
            return Err(EventError::InvalidCursor);
        }
        if self.kinds.is_empty()
            || self.kinds.len() > 10
            || self
                .kinds
                .iter()
                .any(|kind| !EVENT_KINDS.contains(&kind.as_str()))
        {
            return Err(EventError::InvalidCursor);
        }
        if self.kinds != EventFilter::Kinds(self.kinds.clone()).kinds() {
            return Err(EventError::InvalidCursor);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventQuery {
    pub task: TaskId,
    pub limit: usize,
    pub filter: EventFilter,
    pub from_oldest: bool,
    pub cursor: Option<EventCursor>,
}
impl EventQuery {
    pub fn validate(&self) -> Result<(), EventError> {
        if self.limit == 0 || self.limit > 200 {
            return Err(EventError::InvalidLimit);
        }
        self.filter.validate()?;
        if self.from_oldest && self.cursor.is_some() {
            return Err(EventError::InvalidOptions);
        }
        Ok(())
    }
    pub fn validate_cursor(&self, resolved: &TaskId) -> Result<(), EventError> {
        if let Some(cursor) = &self.cursor {
            cursor.validate()?;
            if cursor.kinds != self.filter.kinds() || cursor.task != *resolved {
                return Err(EventError::InvalidCursor);
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureState {
    Live,
    LostWithOwner,
    ObservedTerminal,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogState {
    Unavailable,
    Open,
    Unknown,
    Incomplete,
    Settling,
    Final,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogFacts {
    pub task_status: Option<String>,
    pub available: bool,
    pub capture: CaptureState,
    pub output_drained: Option<bool>,
    pub eof_finished_seq: Option<u64>,
    pub within_grace: bool,
}
impl LogFacts {
    pub fn state(&self) -> LogState {
        if !self.available {
            return LogState::Unavailable;
        }
        match self.capture {
            CaptureState::Live => LogState::Open,
            CaptureState::LostWithOwner => LogState::Incomplete,
            CaptureState::Unknown => LogState::Unknown,
            CaptureState::ObservedTerminal => {
                if self.output_drained == Some(true) {
                    if self.eof_finished_seq.is_some() {
                        return LogState::Final;
                    }
                    if self.within_grace {
                        return LogState::Settling;
                    }
                }
                LogState::Incomplete
            }
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GapReason {
    Unknown,
    UnreadableRecords,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventGap {
    pub after_seq: u64,
    pub before_seq: u64,
    pub reason: GapReason,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn filters_match_pinned_default_all_and_sorted_explicit_semantics() {
        let default = EventFilter::Default.kinds();
        let all = EventFilter::All.kinds();
        assert_eq!(default.len(), EVENT_KINDS.len() - 1);
        assert!(!default.iter().any(|kind| kind == "assistant_delta"));
        assert_eq!(all.len(), EVENT_KINDS.len());
        assert!(all.contains(&String::from("assistant_delta")));
        assert_eq!(
            EventFilter::Kinds(vec![
                String::from("tool_result"),
                String::from("tool_call"),
                String::from("tool_call")
            ])
            .kinds(),
            vec![String::from("tool_call"), String::from("tool_result")]
        );
    }

    #[test]
    fn explicit_filter_rejects_empty_unknown_and_more_than_supported_kinds() {
        assert_eq!(
            EventFilter::Kinds(Vec::new()).validate(),
            Err(EventError::InvalidFilter)
        );
        assert_eq!(
            EventFilter::Kinds(vec![String::from("future")]).validate(),
            Err(EventError::InvalidFilter)
        );
        assert_eq!(
            EventFilter::Kinds(vec![String::from("task_started"); 11]).validate(),
            Err(EventError::InvalidFilter)
        );
    }

    #[test]
    fn log_facts_keep_capture_drain_and_final_evidence_independent() {
        let cases = [
            (
                false,
                CaptureState::Live,
                None,
                None,
                false,
                LogState::Unavailable,
            ),
            (
                true,
                CaptureState::Live,
                Some(true),
                Some(4),
                false,
                LogState::Open,
            ),
            (
                true,
                CaptureState::LostWithOwner,
                Some(true),
                Some(4),
                false,
                LogState::Incomplete,
            ),
            (
                true,
                CaptureState::Unknown,
                Some(true),
                Some(4),
                false,
                LogState::Unknown,
            ),
            (
                true,
                CaptureState::ObservedTerminal,
                Some(false),
                Some(4),
                false,
                LogState::Incomplete,
            ),
            (
                true,
                CaptureState::ObservedTerminal,
                Some(true),
                None,
                true,
                LogState::Settling,
            ),
            (
                true,
                CaptureState::ObservedTerminal,
                Some(true),
                None,
                false,
                LogState::Incomplete,
            ),
            (
                true,
                CaptureState::ObservedTerminal,
                Some(true),
                Some(4),
                true,
                LogState::Final,
            ),
        ];
        for (available, capture, output_drained, eof_finished_seq, within_grace, expected) in cases
        {
            let facts = LogFacts {
                task_status: None,
                available,
                capture,
                output_drained,
                eof_finished_seq,
                within_grace,
            };
            assert_eq!(facts.state(), expected);
        }
    }

    #[test]
    fn query_bounds_limit_and_requires_cursor_filter_match() {
        let task = TaskId::new(String::from("events-task-1234")).unwrap();
        let make = |limit, filter, cursor| EventQuery {
            task: task.clone(),
            limit,
            filter,
            from_oldest: false,
            cursor,
        };
        assert_eq!(
            make(0, EventFilter::Default, None).validate(),
            Err(EventError::InvalidLimit)
        );
        assert_eq!(
            make(201, EventFilter::Default, None).validate(),
            Err(EventError::InvalidLimit)
        );
        assert!(make(200, EventFilter::All, None).validate().is_ok());
        let cursor = EventCursor {
            store: "a".repeat(32),
            generation: "b".repeat(32),
            task: task.clone(),
            kinds: EventFilter::Default.kinds(),
            direction: Direction::Older,
            boundary: 4,
            resync: false,
            neighbour_seq: Some(2),
            neighbour_skip: false,
        };
        assert!(
            make(10, EventFilter::Default, Some(cursor.clone()))
                .validate()
                .is_ok()
        );
        assert!(
            make(10, EventFilter::Default, Some(cursor.clone()))
                .validate_cursor(&task)
                .is_ok()
        );
        assert_eq!(
            make(10, EventFilter::All, Some(cursor)).validate_cursor(&task),
            Err(EventError::InvalidCursor)
        );
    }
}
