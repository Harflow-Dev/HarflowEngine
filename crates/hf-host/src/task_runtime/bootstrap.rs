use super::Error;
use hf_domain::{
    ownership::PreparedRequest,
    task_lifecycle::{LifecycleBinding, LifecycleEvidence},
    task_runtime::{LaunchIntentCreate, ProductionInvitation},
};
use hf_platform::task_runtime::raw::RawRootHold;
use hf_storage::{Limits, RootHold, Store, WriterOptions};
use std::{
    io,
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

/// Shared only inside one process; never transports a connection across spawn.
#[derive(Clone)]
pub struct HeldRoot {
    root: PathBuf,
    hold: Rc<RootHold>,
}
impl HeldRoot {
    /// Acquired before version probing or writer bootstrap. The original lease stays held.
    pub fn open(
        root: &Path,
        invitation: &ProductionInvitation,
        deadline: Instant,
    ) -> Result<Self, Error> {
        let hold = RootHold::open(root, deadline, hf_platform::lease::shared())?;
        if hold.source_stamp() != &invitation.source {
            return Err(Error::Refused);
        }
        hold.revalidate()?;
        Ok(Self {
            root: root.to_owned(),
            hold: Rc::new(hold),
        })
    }
    pub fn revalidate(&self) -> Result<(), Error> {
        self.hold.revalidate().map_err(Error::Storage)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn source(&self) -> &hf_domain::recovery::SourceStamp {
        self.hold.source_stamp()
    }
    /// Dispatcher T0 and the owner's validation open: `Suppressed`, so neither checkpoints.
    pub(super) fn open_validation_store(&self, deadline: Instant) -> Result<Store, Error> {
        self.open_store(deadline, super::store_options::suppressed())
    }
    /// The task writer's handle, held for the task: `Owner` with idle maintenance.
    pub(super) fn open_writer_store(&self, deadline: Instant) -> Result<Store, Error> {
        self.open_store(deadline, super::store_options::writer())
    }
    fn open_store(&self, deadline: Instant, options: WriterOptions) -> Result<Store, Error> {
        self.revalidate()?;
        let store = Store::open_with(
            &self.root,
            Limits::default(),
            deadline,
            hf_platform::lease::shared(),
            options,
        )?;
        if store.identity().generation != self.source().generation.as_str() {
            return Err(Error::Refused);
        }
        self.revalidate()?;
        Ok(store)
    }
}
impl RawRootHold for HeldRoot {
    fn validate(&self, binding: &LifecycleBinding) -> io::Result<()> {
        if binding.generation != self.source().generation {
            return Err(io::Error::other("raw root generation refused"));
        }
        self.revalidate()
            .map_err(|_| io::Error::other("raw original root changed"))
    }
}
/// Exactly one private T0. A failure is not permission to prepare again.
/// The caller must have completed option/caller admission before opening this seam.
pub fn prepare(
    hold: &HeldRoot,
    request: &PreparedRequest,
    intent: &LaunchIntentCreate,
    deadline: Instant,
) -> Result<LifecycleEvidence, Error> {
    let invitation = hf_interfaces::task_runtime::decode_invitation(&request.payload)
        .map_err(|_| Error::Refused)?;
    if &invitation.source != hold.source() {
        return Err(Error::Refused);
    }
    let store = hold.open_validation_store(deadline)?;
    let evidence = store.prepare_launch_intent(
        request,
        intent,
        &hf_interfaces::task_runtime::CanonicalLaunchValidator,
        deadline,
    )?;
    hold.revalidate()?;
    Ok(evidence)
}

#[cfg(test)]
#[path = "bootstrap_tests.rs"]
mod tests;
