//! The statistics projection: finished task executions of a window, their ancestry and the
//! facts a page depends on. Pure policy over the typed task facts of one snapshot.
//!
//! The reference ports it follows walks task ancestry through `continuation_of` only: this
//! engine stores no run, owner or spawn facts, so every task is a standalone execution.
use super::{
    statistics_time::{Bounds, micros_to_seconds},
    statistics_walk::Walk,
    task_rows::ChainRoot,
};
use crate::task_read::TaskId;
use std::collections::{BTreeMap, BTreeSet};

/// Statuses a stored task may have.
pub const KNOWN_STATUSES: [&str; 5] = ["running", "completed", "failed", "cancelled", "timed_out"];
/// Statuses of a finished execution.
pub const FINISHED_STATUSES: [&str; 4] = ["completed", "failed", "cancelled", "timed_out"];
/// Diagnostics a page carries.
const DIAGNOSTIC_LIMIT: usize = 16;
const MESSAGE_CHARS: usize = 512;

/// The facts of one valid task that the projection reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatFacts {
    pub id: TaskId,
    pub status: String,
    pub started_at: String,
    pub started_at_us: i64,
    pub finished_at: Option<String>,
    pub finished_at_us: Option<i64>,
    pub exit_code: Option<i64>,
    pub title: Option<String>,
    pub backend: String,
    pub model: Option<String>,
    pub repo_path: String,
    pub continuation_of: Option<TaskId>,
    pub chain_root: ChainRoot,
}

/// One source of the projection, in id order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatSource {
    Task(Box<StatFacts>),
    /// A source that cannot be projected. It never supplies facts to ancestry, and it makes
    /// the page unavailable.
    Unreadable {
        id: String,
        code: &'static str,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
}

impl Diagnostic {
    /// The message is cut to 512 characters.
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_string(),
            message: message.chars().take(MESSAGE_CHARS).collect(),
        }
    }
}

/// One finished execution of the window.
#[derive(Clone, Debug, PartialEq)]
pub struct StatItem {
    pub task: TaskId,
    pub started_at: f64,
    pub finished_at: f64,
    pub status: String,
    pub repo_path: String,
    pub title: Option<String>,
    pub elapsed_seconds: f64,
    pub continuation_of: Option<TaskId>,
    pub chain_root: ChainRoot,
    pub backend: String,
    pub model: Option<String>,
}

impl StatItem {
    /// `continuation` when the task continues another, else `initial`.
    pub fn execution_kind(&self) -> &'static str {
        if self.continuation_of.is_some() {
            "continuation"
        } else {
            "initial"
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Projection {
    pub items: Vec<StatItem>,
    /// Ids of the tasks the page depends on, in id order.
    pub relevant: Vec<String>,
    pub complete: bool,
    pub diagnostics: Vec<Diagnostic>,
}

/// The deadline ran out while projecting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expired;

/// Seconds of a finished status: `None` unless both ends exist and the end is not earlier.
fn finished_span(facts: &StatFacts) -> Option<(f64, f64)> {
    let start = micros_to_seconds(facts.started_at_us);
    let end = micros_to_seconds(facts.finished_at_us?);
    (end >= start).then_some((start, end))
}

fn item(facts: &StatFacts, start: f64, end: f64) -> StatItem {
    StatItem {
        task: facts.id.clone(),
        started_at: start,
        finished_at: end,
        status: facts.status.clone(),
        repo_path: facts.repo_path.clone(),
        title: facts.title.clone().filter(|title| !title.is_empty()),
        elapsed_seconds: end - start,
        continuation_of: facts.continuation_of.clone(),
        chain_root: facts.chain_root.clone(),
        backend: facts.backend.clone(),
        model: facts.model.clone(),
    }
}

/// What the item pass found.
struct Found {
    relevant: BTreeSet<String>,
    items: Vec<StatItem>,
    fatal: bool,
    diagnostics: Vec<Diagnostic>,
}

impl Found {
    fn note(&mut self, diagnostic: Diagnostic) {
        self.fatal = true;
        if self.diagnostics.len() < DIAGNOSTIC_LIMIT {
            self.diagnostics.push(diagnostic);
        }
    }
}

/// The finished tasks of the window: items for those with a bounded span, the relevant ids
/// and the failures no ancestry could explain.
fn finished(
    walk: &Walk<'_>,
    ordered: &[&StatFacts],
    bounds: &Bounds,
    expired: &dyn Fn() -> bool,
) -> Result<Found, Expired> {
    let mut found = Found {
        relevant: BTreeSet::new(),
        items: Vec::new(),
        fatal: false,
        diagnostics: Vec::new(),
    };
    for facts in ordered {
        if expired() {
            return Err(Expired);
        }
        let id = facts.id.as_str();
        if !FINISHED_STATUSES.contains(&facts.status.as_str()) {
            continue;
        }
        let start = micros_to_seconds(facts.started_at_us);
        let in_window = bounds.since <= start && start < bounds.before;
        let above = walk.ancestry.get(id).cloned().unwrap_or_default();
        if walk.unknown.contains(id) {
            if in_window {
                found.relevant.insert(id.to_string());
                found.relevant.extend(above);
            }
            continue;
        }
        if !in_window {
            continue;
        }
        found.relevant.insert(id.to_string());
        let Some((start, end)) = finished_span(facts) else {
            let message = "Finished task lacks bounded completion timestamps";
            found.note(Diagnostic::new("completion_unavailable", message));
            continue;
        };
        found.relevant.extend(above);
        found.items.push(item(facts, start, end));
    }
    Ok(found)
}

/// Projects the sources, which must be in id order, over `bounds`. `expired` is asked between
/// units of work.
pub fn project(
    sources: &[StatSource],
    bounds: &Bounds,
    expired: &dyn Fn() -> bool,
) -> Result<Projection, Expired> {
    let mut walk = Walk {
        tasks: BTreeMap::new(),
        resolved: BTreeSet::new(),
        unknown: BTreeSet::new(),
        ancestry: BTreeMap::new(),
        issues: Vec::new(),
        expired,
    };
    for source in sources {
        if let StatSource::Task(facts) = source {
            walk.tasks.insert(facts.id.as_str(), facts.as_ref());
        }
    }
    let ordered: Vec<&StatFacts> = walk.tasks.values().copied().collect();
    for facts in &ordered {
        walk.owner(facts.id.as_str(), BTreeSet::new())?;
    }
    let mut found = finished(&walk, &ordered, bounds, expired)?;
    for (scope, diagnostic) in &walk.issues {
        if scope.iter().any(|id| found.relevant.contains(id)) {
            found.note(diagnostic.clone());
        }
    }
    for source in sources {
        if let StatSource::Unreadable { id, code } = source {
            let mut message = String::from("task:");
            message.push_str(id);
            found.note(Diagnostic::new(code, &message));
        }
    }
    let mut items = found.items;
    items.sort_by(|left, right| {
        let order = left.started_at.partial_cmp(&right.started_at);
        order
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.task.as_str().cmp(right.task.as_str()))
    });
    Ok(Projection {
        items,
        relevant: found.relevant.into_iter().collect(),
        complete: !found.fatal,
        diagnostics: found.diagnostics,
    })
}

/// One relevant task as a generation reads it: every fact, or only the identity of an
/// ancestor that has not finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerationEntry {
    pub facts: StatFacts,
    pub identity_only: bool,
}

/// The entries of the relevant ids, in id order.
pub fn generation_entries(sources: &[StatSource], relevant: &[String]) -> Vec<GenerationEntry> {
    let wanted: BTreeSet<&str> = relevant.iter().map(String::as_str).collect();
    let mut found = Vec::new();
    for source in sources {
        if let StatSource::Task(facts) = source
            && wanted.contains(facts.id.as_str())
        {
            found.push(GenerationEntry {
                facts: facts.as_ref().clone(),
                identity_only: !FINISHED_STATUSES.contains(&facts.status.as_str()),
            });
        }
    }
    found
}
