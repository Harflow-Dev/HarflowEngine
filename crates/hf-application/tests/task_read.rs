#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_application::task_read::{
    Authority, TaskAuthorityPort, TaskReadError, TaskReadPort, TaskReadResult, TaskReader,
};
use hf_domain::task_read::{AuthorizedScope, Coverage, PageFilter, TaskId, TaskQuery};
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

struct FixedAuthority {
    result: Authority,
    calls: Rc<Cell<usize>>,
}

impl TaskAuthorityPort for FixedAuthority {
    fn prepare(&self) -> Authority {
        self.calls.set(self.calls.get() + 1);
        self.result.clone()
    }
}

struct CountingRepository {
    calls: Rc<Cell<usize>>,
}

impl TaskReadPort for CountingRepository {
    fn execute(
        &self,
        _scope: &AuthorizedScope,
        _query: &TaskQuery,
        _deadline: Instant,
    ) -> Result<TaskReadResult, TaskReadError> {
        self.calls.set(self.calls.get() + 1);
        Ok(TaskReadResult {
            rows: Vec::new(),
            lookups: Vec::new(),
            cursor: None,
            count: None,
            coverage: Coverage {
                discovery_complete: false,
                ownership_complete: false,
                source_complete: false,
            },
            partial: false,
            activity_complete: false,
        })
    }
}

fn page_query() -> TaskQuery {
    TaskQuery::page(10, PageFilter::new(false, None).unwrap(), None).unwrap()
}

#[test]
fn preparing_authority_returns_without_repository_observation() {
    let authority = FixedAuthority {
        result: Authority::Preparing,
        calls: Rc::new(Cell::new(0)),
    };
    let repository = CountingRepository {
        calls: Rc::new(Cell::new(0)),
    };
    let authority_calls = Rc::clone(&authority.calls);
    let repository_calls = Rc::clone(&repository.calls);
    let reader = TaskReader::new(authority, repository);

    assert_eq!(
        reader.read(&page_query(), deadline()),
        Err(TaskReadError::Preparing)
    );
    assert_eq!(authority_calls.get(), 1);
    assert_eq!(repository_calls.get(), 0);
}

#[test]
fn blocked_authority_refuses_without_repository_observation() {
    let authority = FixedAuthority {
        result: Authority::Blocked,
        calls: Rc::new(Cell::new(0)),
    };
    let repository = CountingRepository {
        calls: Rc::new(Cell::new(0)),
    };
    let authority_calls = Rc::clone(&authority.calls);
    let repository_calls = Rc::clone(&repository.calls);
    let reader = TaskReader::new(authority, repository);

    assert_eq!(
        reader.read(&page_query(), deadline()),
        Err(TaskReadError::Refused)
    );
    assert_eq!(authority_calls.get(), 1);
    assert_eq!(repository_calls.get(), 0);
}

#[test]
fn authorized_closed_scope_is_the_exact_scope_passed_to_repository() {
    let task = TaskId::new("job-12345678".into()).unwrap();
    let scope = AuthorizedScope::Closed(vec![task]);
    let authority = FixedAuthority {
        result: Authority::Authorized(scope.clone()),
        calls: Rc::new(Cell::new(0)),
    };
    let repository = ScopeCheckingRepository {
        expected: scope,
        calls: Rc::new(Cell::new(0)),
    };
    let authority_calls = Rc::clone(&authority.calls);
    let repository_calls = Rc::clone(&repository.calls);
    let reader = TaskReader::new(authority, repository);

    assert!(reader.read(&page_query(), deadline()).is_ok());
    assert_eq!(authority_calls.get(), 1);
    assert_eq!(repository_calls.get(), 1);
}

#[test]
fn oversized_closed_scope_refuses_before_repository_observation() {
    let task = TaskId::new("job-12345678".into()).unwrap();
    let authority = FixedAuthority {
        result: Authority::Authorized(AuthorizedScope::Closed(vec![task; 101])),
        calls: Rc::new(Cell::new(0)),
    };
    let repository = CountingRepository {
        calls: Rc::new(Cell::new(0)),
    };
    let authority_calls = Rc::clone(&authority.calls);
    let repository_calls = Rc::clone(&repository.calls);
    let reader = TaskReader::new(authority, repository);

    assert_eq!(
        reader.read(&page_query(), deadline()),
        Err(TaskReadError::Refused)
    );
    assert_eq!(authority_calls.get(), 1);
    assert_eq!(repository_calls.get(), 0);
}

struct ScopeCheckingRepository {
    expected: AuthorizedScope,
    calls: Rc<Cell<usize>>,
}

impl TaskReadPort for ScopeCheckingRepository {
    fn execute(
        &self,
        scope: &AuthorizedScope,
        _query: &TaskQuery,
        _deadline: Instant,
    ) -> Result<TaskReadResult, TaskReadError> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(scope, &self.expected);
        CountingRepository {
            calls: Rc::new(Cell::new(0)),
        }
        .execute(scope, &page_query(), deadline())
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(1)
}
