//! Composition of the ctl effects: environment, host identity, the state observer and, on
//! macOS arm64, the store-backed reads.
use hf_application::ctl::{
    state_inspect::{StorePolicy, inspect_state},
    task_reads::{ArtifactTargetPort, Reads},
};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use hf_domain::ctl::caller::EnvMarkers;
use hf_domain::{
    ctl::{
        identity::HostIdentity,
        inspect::StateInspection,
        state_root::{InvalidStateRoot, StateRoot},
    },
    process::ProcessIdentity,
};
use hf_interfaces::ctl::{dispatch::CtlServices, state_codec::JsonStateCodec};
use hf_platform::ctl::{state_observer::FsStateObserver, state_root::resolve_state_root};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use super::store_reads::{FsRoots, SnapshotStore};

/// The deadline of one read command.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const READ_BUDGET: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
static SYSTEM_PS: hf_platform::process::PsProcessView =
    hf_platform::process::PsProcessView::system();

/// The services of one command: what an artifact read keeps between its artifacts lives
/// here and goes with the command.
pub struct HostServices {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    artifact_targets: hf_platform::ctl::artifact_target::CommandArtifactTargets,
}

impl HostServices {
    pub fn new() -> Self {
        Self {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            artifact_targets: hf_platform::ctl::artifact_target::CommandArtifactTargets::new(),
        }
    }
}

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
    fn store_reads_supported(&self) -> bool {
        cfg!(all(target_os = "macos", target_arch = "aarch64"))
    }
    fn reads(&self) -> Option<Reads<'_>> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            Some(Reads {
                roots: &FsRoots,
                store: &SnapshotStore,
                process: &SYSTEM_PS,
                markers: EnvMarkers::observe(&|key| {
                    std::env::var_os(key).map(|value| !value.is_empty())
                }),
                deadline: std::time::Instant::now() + READ_BUDGET,
            })
        }
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            None
        }
    }
    fn artifact_targets(&self) -> Option<&dyn ArtifactTargetPort> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            Some(&self.artifact_targets)
        }
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            None
        }
    }
    fn root_task_marker(&self) -> bool {
        std::env::var_os("HARFLOW_ROOT_TASK_ID").is_some_and(|value| !value.is_empty())
    }
    fn now_micros(&self) -> Option<i64> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|elapsed| i64::try_from(elapsed.as_micros()).ok())
    }
    fn observer_parent(&self) -> Option<ProcessIdentity> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            hf_platform::process::current_parent_identity()
        }
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            None
        }
    }
}
