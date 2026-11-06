use crate::{Error, Result, Store, binding::Binding, connection, open_source, wal};
use hf_application::lease::{LeaseMode, LeasePort};
use std::{path::Path, sync::Arc, time::Instant};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    pub busy: u32,
    pub log_frames: i64,
    pub checkpointed_frames: i64,
    pub complete: bool,
}
fn checkpoint(
    connection: &rusqlite::Connection,
    truncate: bool,
    deadline: Instant,
) -> Result<Checkpoint> {
    let sql = if truncate {
        "PRAGMA wal_checkpoint(TRUNCATE)"
    } else {
        "PRAGMA wal_checkpoint(PASSIVE)"
    };
    #[cfg(all(test, unix))]
    crate::tests::stage("before_checkpoint");
    connection::timeout(connection, deadline)?;
    let (busy, log_frames, checkpointed_frames) = connection.query_row(sql, [], |row| {
        Ok((
            row.get::<_, u32>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    #[cfg(all(test, unix))]
    crate::tests::stage("after_checkpoint");
    Ok(Checkpoint {
        busy,
        log_frames,
        checkpointed_frames,
        complete: busy == 0 && log_frames == checkpointed_frames,
    })
}
impl Store {
    pub fn checkpoint_passive(&self, deadline: Instant) -> Result<Checkpoint> {
        let connection = self.admit(deadline)?;
        let _io = self.binding.io(deadline)?;
        connection::check_deadline(deadline)?;
        let result = checkpoint(&connection, false, deadline)?;
        self.binding.check()?;
        Ok(result)
    }
    /// Explicit maintenance refuses while any normal handle exists. No schema migration or reset.
    pub fn recover(
        root: impl AsRef<Path>,
        deadline: Instant,
        scratch_parent: impl AsRef<Path>,
        leases: Arc<dyn LeasePort>,
    ) -> Result<Checkpoint> {
        connection::check_deadline(deadline)?;
        let binding = Binding::capture(root.as_ref(), true, leases)?;
        let _claim = binding.claim(LeaseMode::Exclusive, deadline)?;
        let _io = binding.io(deadline)?;
        let identity = wal::qualify(&binding, scratch_parent.as_ref(), deadline)?;
        let connection = open_source(&binding, &identity, deadline)?;
        let result = checkpoint(&connection, true, deadline)?;
        binding.check()?;
        if !result.complete {
            return Err(Error::Busy);
        }
        Ok(result)
    }
}
