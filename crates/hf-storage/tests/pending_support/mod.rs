#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Launch rows and snapshot helpers of the pending read tests.
use crate::fixture_support::{World, later, tid};
use hf_application::ctl::task_reads::{PendingBudget, PendingLimits};
use hf_domain::ctl::pending::LaunchFacts;
use hf_storage::LaunchRow;

pub const EMPTY_DIGEST: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// A complete, unrefused launch in `state`, named by `name`.
pub fn launch(name: &str, state: &str) -> LaunchRow {
    LaunchRow {
        name: name.to_owned(),
        task_id: Some(tid(name)),
        generation: Some("c".repeat(32)),
        state: Some(state.to_owned()),
        title: Some("Title".into()),
        backend: Some("claude".into()),
        repo_path: Some("/repo".into()),
        created_at: Some("2026-11-10T00:00:00+00:00".into()),
        has_resources: Some(false),
        has_worktree: Some(false),
        ..LaunchRow::default()
    }
}

/// A receipt that does not qualify as a launch document.
pub fn refused(name: &str, code: &str, check: &str) -> LaunchRow {
    LaunchRow {
        name: name.to_owned(),
        task_id: Some(tid(name)),
        refusal_code: Some(code.to_owned()),
        refusal_check: Some(check.to_owned()),
        ..LaunchRow::default()
    }
}

/// A receipt whose name is not a task identity.
pub fn invalid(name: &str) -> LaunchRow {
    LaunchRow {
        name: name.to_owned(),
        ..LaunchRow::default()
    }
}

pub fn put(w: &World, row: &LaunchRow) {
    w.store.put_launch_attempt(row, later(5)).unwrap();
}

/// Up to `count` launch rows after `after`, read one at a time as the page does.
pub fn rows(w: &World, after: Option<&str>, count: usize) -> Vec<LaunchFacts> {
    w.session()
        .snapshot(later(5), |s| {
            let mut found = Vec::new();
            let mut last = after.map(str::to_owned);
            while found.len() < count {
                let Some(name) = s.next_launch_name(last.as_deref())? else {
                    break;
                };
                found.extend(s.pending_row(&tid(&name))?);
                last = Some(name);
            }
            Ok(found)
        })
        .unwrap()
}

pub fn next_name(w: &World, after: Option<&str>) -> Option<String> {
    w.session()
        .snapshot(later(5), |s| s.next_launch_name(after))
        .unwrap()
}

pub fn ids(found: &[LaunchFacts]) -> Vec<&str> {
    found.iter().map(|facts| facts.id.as_str()).collect()
}

pub fn namespace(
    w: &World,
    limits: &PendingLimits,
) -> Option<hf_application::ctl::task_reads::Namespace> {
    w.session()
        .snapshot(later(5), |s| s.pending_namespace(limits))
        .unwrap()
}

pub fn checked(w: &World, read_bytes: u64) -> bool {
    checked_within(w, read_bytes, &PendingLimits::default())
}

pub fn checked_within(w: &World, read_bytes: u64, limits: &PendingLimits) -> bool {
    w.session()
        .snapshot(later(5), |s| {
            s.candidates_checked(&PendingBudget::new(read_bytes), limits)
        })
        .unwrap()
}
