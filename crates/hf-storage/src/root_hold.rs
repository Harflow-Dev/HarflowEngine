//! Root-only ownership hold. Construction may perform filesystem I/O; getters do not.
use crate::{Binding, Error, Identity, Result, connection, wal};
use hf_application::lease::{LeaseGuard, LeaseMode, LeasePort};
use hf_domain::{ownership::Id, recovery::SourceStamp};
use std::{path::Path, sync::Arc, time::Instant};

pub struct RootHold {
    identity: Identity,
    source: SourceStamp,
    // Drop the shared root lease last. There is deliberately no SQLite connection.
    _binding: Binding,
    _claim: Box<dyn LeaseGuard>,
}
impl RootHold {
    pub fn open(
        root: impl AsRef<Path>,
        deadline: Instant,
        leases: Arc<dyn LeasePort>,
    ) -> Result<Self> {
        connection::check_deadline(deadline)?;
        let binding = crate::recovery_gate::open_existing(root.as_ref(), leases)?;
        let claim = binding.claim(LeaseMode::Shared, deadline)?;
        let identity = wal::marker(&binding)?;
        let generation = Id::new(identity.generation.clone()).map_err(|_| Error::Incompatible)?;
        let source = binding.source_stamp(generation)?;
        connection::check_deadline(deadline)?;
        Ok(Self {
            identity,
            source,
            _binding: binding,
            _claim: claim,
        })
    }
    /// Revalidate retained physical source identity without acquiring another lease.
    pub fn revalidate(&self) -> Result<()> {
        self._binding.check()?;
        if self._binding.source_stamp(self.source.generation.clone())? != self.source {
            return Err(Error::Replaced);
        }
        Ok(())
    }
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    pub fn source_stamp(&self) -> &SourceStamp {
        &self.source
    }
}
