use crate::{Result, Store, connection};
use std::time::Instant;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    pub busy: u32,
    pub log_frames: i64,
    pub checkpointed_frames: i64,
    pub complete: bool,
}
pub(crate) fn checkpoint(
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
}
