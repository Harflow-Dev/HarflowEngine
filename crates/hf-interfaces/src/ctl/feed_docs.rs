//! The documents of recovery-activity, interaction-events and statistics-page, and the sizes
//! the statistics page is held to.
use super::{
    bound::{compact_size_with, python_size_with, utf8_size},
    feed_cursor,
    history_docs::iso_micros,
    py_float::Exact,
};
use hf_application::ctl::task_reads::{FeedPage, RecoveryOut, StatsMeta, StatsOut, StatsSizing};
use hf_domain::ctl::{
    feed::FeedEvent,
    recovery::notice,
    statistics::{Diagnostic, StatFacts, StatItem},
    statistics_time::{Bounds, Retention},
};
use serde_json::{Value, json};

/// A qualified task payload over this many bytes is not projected.
const PAYLOAD_BYTES: usize = 128 * 1024;
const NO_CLOCK: &str = "1970-01-01T00:00:00+00:00";

pub fn recovery_body(out: &RecoveryOut) -> Value {
    let page = json!({
        "source": "backend_transcript",
        "display_only": true,
        "events": [],
        "next_cursor": null,
        "has_more": false,
        "reset": false,
        "notices": [notice(out.capture, &out.backend)],
    });
    let mut body = json!({"v": 1, "task_id": out.task.as_str(), "recovery_activity": page});
    if let (Some(input), Some(map)) = (&out.resolved_from, body.as_object_mut()) {
        map.insert(
            "resolved_from".into(),
            json!({"input": input, "task_id": out.task.as_str()}),
        );
    }
    body
}

fn event_value(event: &FeedEvent) -> Value {
    let task = event.task.as_str();
    json!({
        "event_id": format!("task:{task}:{}", event.state),
        "interaction_id": task,
        "kind": "task",
        "state": event.state,
        "task_id": task,
        "workflow_run_id": null,
        "source_workflow_run_id": null,
        "title": event.title,
        "occurred_at": event.occurred_at,
    })
}

/// The bytes the events take in the document: `json.dumps(events, ensure_ascii=False)`.
pub fn feed_size(events: &[FeedEvent]) -> usize {
    let values: Vec<Value> = events.iter().map(event_value).collect();
    utf8_size(&Value::Array(values))
}

pub fn feed_result(page: &FeedPage) -> Value {
    let events: Vec<Value> = page.events.iter().map(event_value).collect();
    let gaps: Vec<Value> = page
        .gaps
        .iter()
        .map(|gap| json!({"source": gap.source, "reason": gap.reason}))
        .collect();
    json!({
        "events": events,
        "next_cursor": page.next.as_ref().map(feed_cursor::encode_feed),
        "cycle_complete": page.next.is_none(),
        "history_incomplete": !page.gaps.is_empty(),
        "gaps": gaps,
    })
}

fn retention_value(retention: &Retention) -> Value {
    match retention {
        Retention::Unknown => json!({"state": "unknown", "days": null}),
        Retention::Disabled => json!({"state": "disabled", "days": null}),
        // A count beyond u64 is written out of band, at `/retention/days`.
        Retention::Days(digits) => {
            let days = digits
                .parse::<u64>()
                .map_or(Value::Null, |days| json!(days));
            json!({"state": "days", "days": days})
        }
    }
}

fn diagnostics_value(diagnostics: &[Diagnostic]) -> Value {
    let rows: Vec<Value> = diagnostics
        .iter()
        .map(|found| json!({"code": found.code, "message": found.message}))
        .collect();
    Value::Array(rows)
}

fn unavailable_accounting(task: &str) -> Value {
    json!({
        "version": 1,
        "contribution_id": format!("task:{task}"),
        "state": "unavailable",
        "scope": "unknown",
        "additive": false,
        "categories": {
            "input_tokens": null,
            "output_tokens": null,
            "cache_read_tokens": null,
            "cache_creation_tokens": null,
        },
        "total_tokens": null,
        "qualified_wait_intervals": [],
        "diagnostics": ["legacy_missing_accounting"],
    })
}

fn link(id: &Option<hf_domain::task_read::TaskId>) -> Value {
    id.as_ref().map_or(Value::Null, |id| json!(id.as_str()))
}

fn item_value(item: &StatItem) -> Value {
    let task = item.task.as_str();
    let value = json!({
        "id": format!("execution:{task}"),
        "kind": "execution",
        "cohort_started_at": item.started_at,
        "started_at": item.started_at,
        "finished_at": item.finished_at,
        "status": item.status,
        "repo_path": item.repo_path,
        "title": item.title,
        "root_workflow_run_id": null,
        "workflow_run_id": null,
        "workflow_id": null,
        "workflow_revision": null,
        "workflow_name": null,
        "elapsed_seconds": item.elapsed_seconds,
        "execution_seconds": item.elapsed_seconds,
        "wait_seconds": 0,
        "time_qualification": "elapsed_may_include_waits",
        "task_id": task,
        "parent_task_id": null,
        "continuation_of_task_id": link(&item.continuation_of),
        "chain_root_task_id": link(&item.chain_root),
        "node_id": null,
        "activation_id": null,
        "requested_harness": item.backend,
        "requested_model": item.model,
        "accounting": unavailable_accounting(task),
        "execution_kind": item.execution_kind(),
        "duration_additive": true,
        "usage_additive": false,
        "owner_task_id": null,
        "cost_usd": null,
        "cost_provenance": null,
        "cost_additive": false,
        "observed_model": null,
        "observed_model_provenance": null,
    });
    value
}

/// What a statistics document needs besides the page: the clock, the policy it reports and
/// the root and window its cursors bind.
pub struct StatsDoc<'a> {
    pub root: &'a str,
    pub bounds: Option<Bounds>,
    pub now: Option<i64>,
    pub retention: Retention,
}

fn stamp(now: Option<i64>) -> String {
    now.map_or_else(|| NO_CLOCK.to_string(), iso_micros)
}

impl StatsDoc<'_> {
    /// The retention days when they do not fit u64, to be written out of band at `pointer`.
    pub fn exact(&self, pointer: &'static str) -> Option<Exact<'_>> {
        match &self.retention {
            Retention::Days(digits) if digits.parse::<u64>().is_err() => {
                Some(Exact { pointer, digits })
            }
            _ => None,
        }
    }

    fn ready(&self, meta: &StatsMeta, items: &[StatItem], next: Option<&str>) -> Value {
        let coverage = json!({"eligible_executions": meta.eligible, "available_executions": 0});
        json!({
            "projection_version": 1,
            "state": "ready",
            "snapshot_id": meta.generation,
            "generation": meta.generation,
            "observed_at": stamp(self.now),
            "items": items.iter().map(item_value).collect::<Vec<Value>>(),
            "next_cursor": next,
            "has_more": next.is_some(),
            "history_complete": true,
            "ownership_complete": true,
            "accounting_coverage": coverage.clone(),
            "cost_coverage": coverage,
            "retained_history_only": true,
            "retention": retention_value(&self.retention),
            "diagnostics": diagnostics_value(&meta.diagnostics),
        })
    }

    /// The result of the read. The stamp is the time of completion.
    pub fn result(&self, out: &StatsOut, finished: Option<i64>) -> Value {
        let mut value = match out {
            StatsOut::Ready(page) => {
                self.ready(&page.meta, &page.items, page.next_cursor.as_deref())
            }
            StatsOut::Unavailable(diagnostics) | StatsOut::Preparing(diagnostics) => {
                let state = if matches!(out, StatsOut::Preparing(_)) {
                    "preparing"
                } else {
                    "unavailable"
                };
                let none = json!({"eligible_executions": 0, "available_executions": 0});
                json!({
                    "projection_version": 1,
                    "state": state,
                    "snapshot_id": null,
                    "generation": null,
                    "observed_at": stamp(self.now),
                    "items": [],
                    "next_cursor": null,
                    "has_more": false,
                    "history_complete": false,
                    "ownership_complete": false,
                    "accounting_coverage": none.clone(),
                    "cost_coverage": none,
                    "retained_history_only": true,
                    "retention": retention_value(&self.retention),
                    "diagnostics": diagnostics_value(diagnostics),
                })
            }
        };
        if let (Some(map), StatsOut::Ready(_)) = (value.as_object_mut(), out) {
            map.insert("observed_at".into(), json!(stamp(finished)));
        }
        value
    }
}

/// The qualified payload of a task as the reference keeps it, for the bound on one source.
fn payload(facts: &StatFacts) -> Value {
    json!({
        "task_id": facts.id.as_str(),
        "backend": facts.backend,
        "model": facts.model,
        "repo_path": facts.repo_path,
        "started_at": facts.started_at,
        "finished_at": facts.finished_at,
        "status": facts.status,
        "title": facts.title,
        "parent_task_id": null,
        "continuation_of_task_id": link(&facts.continuation_of),
        "chain_root_task_id": link(&facts.chain_root),
        "spawned_by": null,
        "root_task_id": null,
        "accounting": null,
        "exit_code": facts.exit_code,
        "workflow_builder": false,
        "total_cost_usd": null,
        "observed_model": null,
    })
}

impl StatsSizing for StatsDoc<'_> {
    fn source_oversized(&self, facts: &StatFacts) -> bool {
        compact_size_with(&payload(facts), None) > PAYLOAD_BYTES
    }
    fn candidate(&self, meta: &StatsMeta, items: &[StatItem]) -> usize {
        compact_size_with(
            &self.ready(meta, items, None),
            self.exact("/retention/days"),
        )
    }
    fn cursor(&self, generation: &str, offset: usize) -> String {
        self.bounds.as_ref().map_or_else(String::new, |window| {
            feed_cursor::encode_statistics(self.root, window, generation, offset)
        })
    }
    fn document(&self, meta: &StatsMeta, items: &[StatItem], cursor: Option<&str>) -> usize {
        python_size_with(
            &self.ready(meta, items, cursor),
            self.exact("/retention/days"),
        )
    }
}
