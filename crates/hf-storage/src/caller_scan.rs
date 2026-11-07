//! Candidate callers for the gate: unfinished tasks related to the calling process by
//! recorded session or by an ancestor's pid.
use crate::{Error, Result, snapshot::TaskSnapshot, statements, task_catalog::within};
use hf_domain::{
    ctl::task_rows::{CallerRow, ProcessRecord},
    process::ProcessIdentity,
    task_read::TaskId,
};
use rusqlite::{params_from_iter, types::Value};

const BATCH: usize = 100;

/// The recorded process of a row; an identity that cannot be decided is `None`.
pub(crate) fn process_record(
    pid: i64,
    seconds: Option<i64>,
    micros: Option<i64>,
    boot: Option<String>,
) -> Result<ProcessRecord> {
    let pid = u32::try_from(pid).map_err(|_| Error::MalformedRow)?;
    let start = match (seconds, micros, boot) {
        (Some(seconds), Some(micros), Some(boot)) => {
            match (u64::try_from(seconds), u64::try_from(micros)) {
                (Ok(seconds), Ok(micros)) => ProcessIdentity::new(pid, seconds, micros, boot).ok(),
                _ => None,
            }
        }
        _ => None,
    };
    Ok(ProcessRecord { pid, start })
}

impl TaskSnapshot<'_> {
    /// With `Some(limit)` at most `limit + 1` rows are returned, enough to tell a bounded
    /// family that it overflowed. Without a limit every related row is returned, in
    /// batches, bounded by the deadline.
    pub fn related_callers(
        &self,
        session: i64,
        ancestors: &[u32],
        limit: Option<usize>,
    ) -> Result<Vec<CallerRow>> {
        let sql = statements::related_callers(ancestors.len());
        let mut rows = Vec::new();
        let mut last = String::new();
        loop {
            let wanted = limit.map_or(BATCH, |cap| (cap + 1).saturating_sub(rows.len()).min(BATCH));
            if wanted == 0 {
                break;
            }
            self.step()?;
            let mut params = vec![Value::Integer(session)];
            params.extend(ancestors.iter().map(|pid| Value::Integer(i64::from(*pid))));
            params.push(Value::Text(last.clone()));
            params.push(Value::Integer(wanted as i64));
            let mut statement = self.db.prepare(&sql)?;
            let mut found = statement.query(params_from_iter(params.iter()))?;
            let (mut count, mut bytes) = (0usize, 0usize);
            while let Some(row) = found.next()? {
                let status_len = within(row.get(7)?, 256)?;
                let boot_len = within(row.get(8)?, 64)?;
                bytes += status_len + boot_len + 64;
                self.context.check_bounds(count + 1, bytes)?;
                let id: String = row.get(0)?;
                last.clone_from(&id);
                let mut process =
                    process_record(row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)?;
                // A process row whose task row is missing cannot be judged.
                if row.get::<_, bool>(9)? {
                    process.start = None;
                }
                rows.push(CallerRow {
                    task: TaskId::new(id).map_err(|_| Error::MalformedRow)?,
                    status: row.get(1)?,
                    exit_code: row.get(2)?,
                    process,
                });
                count += 1;
            }
            if count < wanted {
                break;
            }
        }
        Ok(rows)
    }
}
