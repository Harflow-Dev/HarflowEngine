#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_application::{
    task_read::{Authority, TaskAuthorityPort},
    task_watch::{TaskWatchPort, TaskWatcher, WatchError},
};
use hf_domain::{
    task_read::{AuthorizedScope, TaskId},
    task_watch::{WatchReason, WatchRequest, WatchResult, WatchState},
};
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

struct AuthorityStub(Authority, Rc<Cell<usize>>);
impl TaskAuthorityPort for AuthorityStub {
    fn prepare(&self) -> Authority {
        self.1.set(self.1.get() + 1);
        self.0.clone()
    }
}
struct RepositoryStub(Rc<Cell<usize>>);
impl TaskWatchPort for RepositoryStub {
    fn execute(&self, _: &WatchRequest, _: Instant) -> Result<WatchResult, WatchError> {
        self.0.set(self.0.get() + 1);
        Ok(WatchResult {
            state: WatchState::Unavailable,
            cursor: None,
            reason: Some(WatchReason::StateMissing),
        })
    }
}
type WatcherFixture = (
    TaskWatcher<AuthorityStub, RepositoryStub>,
    Rc<Cell<usize>>,
    Rc<Cell<usize>>,
);

fn watcher(authority: Authority) -> WatcherFixture {
    let authority_calls = Rc::new(Cell::new(0));
    let repository_calls = Rc::new(Cell::new(0));
    (
        TaskWatcher::new(
            AuthorityStub(authority, authority_calls.clone()),
            RepositoryStub(repository_calls.clone()),
        ),
        authority_calls,
        repository_calls,
    )
}
fn request(wait_seconds: u32) -> WatchRequest {
    WatchRequest {
        cursor: Some("opaque-cursor".into()),
        wait_seconds,
    }
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(1)
}

#[test]
fn direct_global_authority_precedes_validation_and_repository_access() {
    let (watcher, authority, repository) = watcher(Authority::Authorized(AuthorizedScope::Global));
    assert!(watcher.watch(&request(31), deadline()).is_err());
    assert_eq!(authority.get(), 1);
    assert_eq!(repository.get(), 0);
    assert!(watcher.watch(&request(0), deadline()).is_ok());
    assert_eq!(repository.get(), 1);
}

#[test]
fn preparing_and_closed_scopes_refuse_before_any_repository_work() {
    let id = TaskId::new("task-12345678".to_owned()).unwrap();
    for authority_value in [
        Authority::Preparing,
        Authority::Authorized(AuthorizedScope::Closed(vec![id])),
    ] {
        let (watcher, authority, repository) = watcher(authority_value);
        assert!(watcher.watch(&request(0), deadline()).is_err());
        assert_eq!(authority.get(), 1);
        assert_eq!(repository.get(), 0);
    }
}
