//! Continuation-stop markers (schema 7), one row per chain root. A `marker` row is a stopped
//! chain whose attribution is the raw stored text; a `fault` row is a marker the engine
//! cannot establish. No row means not stopped.
//!
//! Attribution rule for readers: only text that is a valid task identity (at most 64
//! characters of the identity alphabet) is an attribution; any other text, and `NULL`,
//! projects to a stopped chain with a null `cancelled_task_id`. The reference engine's
//! oversize threshold applies to the whole marker document (more than 4096 bytes,
//! `ATTRIBUTION_BYTES`); the seed stores the attribution followed by 4096 `x`, which is
//! never an identity, so an oversize marker projects to null attribution as well. Writes never change task-list metadata or
//! the paging generation.
use crate::{Result, Store};
use hf_domain::task_read::TaskId;
use std::time::Instant;

pub(crate) const SCHEMA: &str = "
CREATE TABLE chain_stops(
  root TEXT PRIMARY KEY NOT NULL CHECK(typeof(root)='text' AND length(root) BETWEEN 1 AND 64),
  kind TEXT NOT NULL CHECK(kind IN('marker','fault')),
  cancelled_task_id TEXT CHECK(cancelled_task_id IS NULL OR typeof(cancelled_task_id)='text'),
  CHECK(kind='marker' OR cancelled_task_id IS NULL)
);
";
/// What a stored chain-stop row says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChainStop {
    /// The marker exists. `cancelled_task_id` is the raw attribution text: malformed,
    /// oversize and invalid values are representable, and `None` means none could be read.
    Marker { cancelled_task_id: Option<String> },
    /// The marker name exists but the engine cannot establish presence or absence.
    Fault,
}
impl Store {
    /// Sets the stop state of `root`. An existing marker or fault refuses the write, as the
    /// reference adapter does; `remove_chain_stop` is idempotent.
    pub fn put_chain_stop(&self, root: &TaskId, stop: &ChainStop, deadline: Instant) -> Result<()> {
        let (kind, cancelled) = match stop {
            ChainStop::Marker { cancelled_task_id } => ("marker", cancelled_task_id.as_deref()),
            ChainStop::Fault => ("fault", None),
        };
        self.check_bounds(1, root.as_str().len() + cancelled.map_or(0, str::len))?;
        self.fixture_write(deadline, |tx| {
            tx.execute(
                "INSERT INTO chain_stops VALUES(?1,?2,?3)",
                rusqlite::params![root.as_str(), kind, cancelled],
            )?;
            Ok(())
        })
    }
    /// Removes the stop state of `root`; absent stays absent.
    pub fn remove_chain_stop(&self, root: &TaskId, deadline: Instant) -> Result<()> {
        self.fixture_write(deadline, |tx| {
            tx.execute("DELETE FROM chain_stops WHERE root=?1", [root.as_str()])?;
            Ok(())
        })
    }
}
