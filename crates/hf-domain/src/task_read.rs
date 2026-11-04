//! Internal stored-observation query values; no public wire semantics.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct TaskId(String);
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueryError {
    InvalidId,
    InvalidLimit,
    InvalidSession,
    TooManyIds,
    InvalidPrefix,
    StaleCursor,
    InvalidScope,
}
impl TaskId {
    pub fn new(value: String) -> Result<Self, QueryError> {
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .enumerate()
                .all(|(i, b)| b.is_ascii_alphanumeric() || (i > 0 && (b == b'_' || b == b'-')));
        if valid {
            Ok(Self(value))
        } else {
            Err(QueryError::InvalidId)
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn canonical(&self) -> bool {
        let bytes = self.0.as_bytes();
        (bytes.len() == 32 && bytes.iter().all(u8::is_ascii_hexdigit))
            || (bytes.len() == 36
                && bytes.iter().enumerate().all(|(i, b)| {
                    if i == 8 || i == 13 || i == 18 || i == 23 {
                        *b == b'-'
                    } else {
                        b.is_ascii_hexdigit()
                    }
                }))
    }
    pub fn may_prefix(&self) -> bool {
        self.0.len() >= 8 && !self.canonical()
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthorizedScope {
    Global,
    Closed(Vec<TaskId>),
}
impl AuthorizedScope {
    pub fn validate(&self) -> Result<(), QueryError> {
        if let Self::Closed(ids) = self
            && ids.len() > 100
        {
            return Err(QueryError::InvalidScope);
        }
        Ok(())
    }
    pub fn permits(&self, id: &TaskId) -> bool {
        match self {
            Self::Global => true,
            Self::Closed(ids) => ids.contains(id),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageFilter {
    pub active_only: bool,
    pub session: Option<String>,
}
impl PageFilter {
    pub fn validate(&self) -> Result<(), QueryError> {
        if self
            .session
            .as_ref()
            .is_some_and(|s| s.len() > 1024 || s.chars().take(257).count() > 256)
        {
            return Err(QueryError::InvalidSession);
        }
        Ok(())
    }
    pub fn new(active_only: bool, session: Option<String>) -> Result<Self, QueryError> {
        if session
            .as_ref()
            .is_some_and(|s| s.len() > 1024 || s.chars().take(257).count() > 256)
        {
            return Err(QueryError::InvalidSession);
        }
        Ok(Self {
            active_only,
            session,
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SortKey {
    pub stamp: i64,
    pub id: TaskId,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cursor {
    pub store: String,
    pub dataset: String,
    pub scope: AuthorizedScope,
    pub filter: PageFilter,
    pub after: SortKey,
}
impl Cursor {
    pub fn validate(&self) -> Result<(), QueryError> {
        if self.store.len() != 32
            || self.dataset.len() != 32
            || !self.store.bytes().all(|b| b.is_ascii_hexdigit())
            || !self.dataset.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(QueryError::StaleCursor);
        }
        self.scope.validate()?;
        self.filter.validate()
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskQuery {
    StoredStatus(TaskId),
    Page {
        limit: usize,
        filter: PageFilter,
        cursor: Option<Cursor>,
    },
    Ids(Vec<TaskId>),
}
impl TaskQuery {
    pub fn page(
        limit: usize,
        filter: PageFilter,
        cursor: Option<Cursor>,
    ) -> Result<Self, QueryError> {
        if limit == 0 || limit > 100 {
            return Err(QueryError::InvalidLimit);
        }
        filter.validate()?;
        if let Some(cursor) = &cursor {
            cursor.validate()?;
        }
        Ok(Self::Page {
            limit,
            filter,
            cursor,
        })
    }
    pub fn ids(ids: Vec<TaskId>) -> Result<Self, QueryError> {
        if ids.len() > 100 {
            return Err(QueryError::TooManyIds);
        }
        let mut unique = Vec::new();
        for id in ids {
            if !unique.contains(&id) {
                unique.push(id);
            }
        }
        Ok(Self::Ids(unique))
    }
    pub fn validate(&self) -> Result<(), QueryError> {
        match self {
            Self::StoredStatus(_) => {}
            Self::Page {
                limit,
                filter,
                cursor,
            } => {
                if *limit == 0 || *limit > 100 {
                    return Err(QueryError::InvalidLimit);
                }
                filter.validate()?;
                if let Some(cursor) = cursor {
                    cursor.validate()?;
                }
            }
            Self::Ids(ids) => {
                if ids.len() > 100 {
                    return Err(QueryError::TooManyIds);
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Coverage {
    pub discovery_complete: bool,
    pub ownership_complete: bool,
    pub source_complete: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredObservation {
    pub recorded_status: Option<String>,
    pub active: Option<bool>,
    pub exited: Option<bool>,
    pub drained: Option<bool>,
    pub owned: Option<bool>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Lookup {
    Found(TaskId),
    Pending(TaskId),
    Missing,
    InvalidPrefix,
    Incomplete,
    Ambiguous {
        candidates: Vec<TaskId>,
        truncated: bool,
    },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{AuthorizedScope, Cursor, PageFilter, QueryError, SortKey, TaskId, TaskQuery};

    fn id(value: &str) -> TaskId {
        TaskId::new(value.to_owned()).unwrap()
    }

    #[test]
    fn task_ids_enforce_ascii_shape_and_length() {
        assert!(TaskId::new("A".into()).is_ok());
        assert!(TaskId::new(format!("a{}", "x".repeat(63))).is_ok());
        assert_eq!(TaskId::new(String::new()), Err(QueryError::InvalidId));
        assert_eq!(TaskId::new("_leading".into()), Err(QueryError::InvalidId));
        assert_eq!(
            TaskId::new(format!("a{}", "x".repeat(64))),
            Err(QueryError::InvalidId)
        );
        assert_eq!(TaskId::new("café1234".into()), Err(QueryError::InvalidId));
    }

    #[test]
    fn canonical_uuid_spellings_never_become_prefixes() {
        for value in [
            "0123456789abcdef0123456789abcdef",
            "01234567-89ab-cdef-0123-456789abcdef",
        ] {
            let parsed = id(value);
            assert!(parsed.canonical());
            assert!(!parsed.may_prefix());
        }
        assert!(!id("shortid").may_prefix());
        assert!(id("task1234").may_prefix());
        assert!(id("prefix-123").may_prefix());
    }

    #[test]
    fn explicit_ids_deduplicate_without_reordering() {
        let query = TaskQuery::ids(vec![id("first-id"), id("second-id"), id("first-id")]).unwrap();
        assert_eq!(query, TaskQuery::Ids(vec![id("first-id"), id("second-id")]));
        assert_eq!(
            TaskQuery::ids(vec![id("x"); 101]),
            Err(QueryError::TooManyIds)
        );
    }

    #[test]
    fn page_bounds_and_session_length_are_checked() {
        let filter = PageFilter::new(false, None).unwrap();
        assert_eq!(
            TaskQuery::page(1, filter.clone(), None).unwrap(),
            TaskQuery::Page {
                limit: 1,
                filter: filter.clone(),
                cursor: None
            }
        );
        assert!(matches!(
            TaskQuery::page(0, filter.clone(), None),
            Err(QueryError::InvalidLimit)
        ));
        assert!(matches!(
            TaskQuery::page(101, filter, None),
            Err(QueryError::InvalidLimit)
        ));
        assert!(PageFilter::new(false, Some("s".repeat(256))).is_ok());
        assert_eq!(
            PageFilter::new(false, Some("s".repeat(257))),
            Err(QueryError::InvalidSession)
        );
    }

    #[test]
    fn cursors_require_internal_identity_and_bounded_scope_filter() {
        let after = SortKey {
            stamp: 7,
            id: id("cursor-id"),
        };
        let valid = Cursor {
            store: "a".repeat(32),
            dataset: "b".repeat(32),
            scope: AuthorizedScope::Global,
            filter: PageFilter::new(false, None).unwrap(),
            after: after.clone(),
        };
        assert!(valid.validate().is_ok());
        let malformed = Cursor {
            store: "bad".into(),
            ..valid.clone()
        };
        assert_eq!(malformed.validate(), Err(QueryError::StaleCursor));
        let too_many = Cursor {
            scope: AuthorizedScope::Closed(vec![id("x"); 101]),
            ..valid.clone()
        };
        assert_eq!(too_many.validate(), Err(QueryError::InvalidScope));
        let oversized_filter = Cursor {
            filter: PageFilter {
                active_only: false,
                session: Some("x".repeat(1025)),
            },
            ..valid
        };
        assert_eq!(oversized_filter.validate(), Err(QueryError::InvalidSession));
    }
}
