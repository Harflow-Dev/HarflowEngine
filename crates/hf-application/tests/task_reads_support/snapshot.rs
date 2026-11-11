//! The snapshot port of the scripted world.
use super::World;
use hf_application::ctl::task_reads::{StoreFailure, TaskSnapshotPort};
use hf_domain::{
    ctl::{
        continuation_stop::StopRow,
        task_rows::{BatchQuery, CallerRow, TaskRecord, TaskSummary, UnobservedRow},
    },
    task_read::{Lookup, PendingLaunch, TaskId},
};

impl TaskSnapshotPort for World {
    fn damaged(&self) -> Result<bool, StoreFailure> {
        self.note("damaged".into());
        Ok(self.damaged)
    }
    fn related_callers(
        &self,
        session: i64,
        ancestors: &[u32],
        limit: Option<usize>,
    ) -> Result<Vec<CallerRow>, StoreFailure> {
        self.note(format!(
            "related:{session}:{}:{}:{}-{}",
            ancestors.len(),
            limit.map_or("none".to_string(), |cap| cap.to_string()),
            ancestors.first().map_or(0, |pid| *pid),
            ancestors.last().map_or(0, |pid| *pid)
        ));
        let mut rows = self.callers.clone();
        if let Some(cap) = limit {
            rows.truncate(cap + 1);
        }
        Ok(rows)
    }
    fn lookup(&self, id: &TaskId) -> Result<Lookup, StoreFailure> {
        self.note(format!("lookup:{}", id.as_str()));
        if self.pending.contains(id) {
            return Ok(Lookup::Pending(PendingLaunch {
                id: id.clone(),
                state: "waiting_for_resources".into(),
                not_started_reason: None,
            }));
        }
        if self.tasks.iter().any(|task| &task.facts.id == id) {
            return Ok(Lookup::Found(id.clone()));
        }
        if id.may_prefix() {
            let hits: Vec<TaskId> = self
                .tasks
                .iter()
                .map(|task| task.facts.id.clone())
                .filter(|found| found.as_str().starts_with(id.as_str()))
                .collect();
            return Ok(match hits.len() {
                0 => Lookup::Missing,
                1 => Lookup::Found(hits[0].clone()),
                _ => Lookup::Ambiguous {
                    candidates: hits,
                    truncated: false,
                },
            });
        }
        Ok(Lookup::Missing)
    }
    fn summaries(&self, ids: &[TaskId]) -> Result<Vec<TaskSummary>, StoreFailure> {
        Ok(self
            .tasks
            .iter()
            .filter(|task| ids.contains(&task.facts.id))
            .map(|task| TaskSummary {
                id: task.facts.id.clone(),
                title: task.facts.title.clone(),
                status: task.facts.status.clone(),
                created_at: task.facts.started_at.clone(),
            })
            .collect())
    }
    fn record(&self, id: &TaskId) -> Result<Option<TaskRecord>, StoreFailure> {
        self.note(format!("record:{}", id.as_str()));
        if self.vanish.contains(id) {
            return Ok(None);
        }
        Ok(self.tasks.iter().find(|task| &task.facts.id == id).cloned())
    }
    fn record_with_stop(&self, id: &TaskId) -> Result<Option<(TaskRecord, StopRow)>, StoreFailure> {
        self.note(format!("record_stop:{}", id.as_str()));
        if self.vanish.contains(id) {
            return Ok(None);
        }
        let row = self
            .stops
            .iter()
            .find(|(task, _)| task == id)
            .map_or(StopRow::Absent, |(_, row)| row.clone());
        Ok(self
            .tasks
            .iter()
            .find(|task| &task.facts.id == id)
            .cloned()
            .map(|record| (record, row)))
    }
    fn batch(&self, query: &BatchQuery) -> Result<Vec<TaskRecord>, StoreFailure> {
        self.note("batch".into());
        let mut rows: Vec<TaskRecord> = self
            .tasks
            .iter()
            .filter(|task| {
                let unobserved = task.facts.status == "running" || task.facts.exit_code.is_none();
                (!query.unobserved_only || unobserved)
                    && query
                        .since_us
                        .is_none_or(|since| task.facts.started_at_us >= since)
                    && query.after.as_ref().is_none_or(|(stamp, id)| {
                        (task.facts.started_at_us, task.facts.id.as_str().to_string())
                            < (*stamp, id.clone())
                    })
            })
            .cloned()
            .collect();
        rows.sort_by(|a, b| {
            (b.facts.started_at_us, b.facts.id.as_str())
                .cmp(&(a.facts.started_at_us, a.facts.id.as_str()))
        });
        rows.truncate(query.max_rows);
        Ok(rows)
    }
    fn unobserved_count(&self) -> Result<u64, StoreFailure> {
        self.note("unobserved_count".into());
        Ok(self.unobserved_total.unwrap_or_else(|| {
            self.tasks
                .iter()
                .filter(|task| task.facts.status == "running" || task.facts.exit_code.is_none())
                .count() as u64
        }))
    }
    fn unobserved_rows(
        &self,
        after: Option<(i64, String)>,
        max_rows: usize,
    ) -> Result<Vec<UnobservedRow>, StoreFailure> {
        self.note("unobserved_rows".into());
        let mut rows: Vec<&TaskRecord> = self
            .tasks
            .iter()
            .filter(|task| task.facts.status == "running" || task.facts.exit_code.is_none())
            .filter(|task| {
                after.as_ref().is_none_or(|(stamp, id)| {
                    (task.facts.started_at_us, task.facts.id.as_str().to_string())
                        < (*stamp, id.clone())
                })
            })
            .collect();
        rows.sort_by(|a, b| {
            (b.facts.started_at_us, b.facts.id.as_str())
                .cmp(&(a.facts.started_at_us, a.facts.id.as_str()))
        });
        Ok(rows
            .into_iter()
            .take(max_rows)
            .map(|task| UnobservedRow {
                id: task.facts.id.clone(),
                stamp: task.facts.started_at_us,
                session: task.facts.session_id.clone(),
                status: task.facts.status.clone(),
                exit_code: task.facts.exit_code,
                process: task.process.clone(),
            })
            .collect())
    }
    fn cursor_scope(
        &self,
        active_only: bool,
        session: Option<&str>,
        predecessor: Option<&TaskId>,
    ) -> Result<String, StoreFailure> {
        Ok(format!(
            "{active_only}:{}:{}",
            session.unwrap_or("-"),
            predecessor.map_or("-", TaskId::as_str)
        ))
    }
}
