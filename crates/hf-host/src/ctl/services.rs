//! Composition of the ctl effects: environment, host identity and the state observer.
use hf_application::ctl::state_inspect::{StorePolicy, inspect_state};
use hf_domain::ctl::{
    identity::HostIdentity,
    inspect::StateInspection,
    state_root::{InvalidStateRoot, StateRoot},
};
use hf_interfaces::ctl::{dispatch::CtlServices, state_codec::JsonStateCodec};
use hf_platform::ctl::{state_observer::FsStateObserver, state_root::resolve_state_root};
pub struct HostServices;
impl CtlServices for HostServices {
    fn resolve_state_root(&self) -> Result<StateRoot, InvalidStateRoot> {
        resolve_state_root(std::env::var_os("HARFLOW_STATE_ROOT"))
    }
    fn host_identity(&self) -> HostIdentity {
        HostIdentity::from_rust(std::env::consts::OS, std::env::consts::ARCH)
    }
    fn inspect(&self, root: &StateRoot) -> StateInspection {
        let policy = StorePolicy {
            schema_version: hf_storage::SCHEMA_VERSION,
            application_id: hf_storage::APPLICATION_ID,
        };
        inspect_state(&FsStateObserver, &JsonStateCodec, policy, root)
    }
}
