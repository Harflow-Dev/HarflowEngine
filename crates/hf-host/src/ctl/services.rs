//! Composition of the ctl effects: environment, host identity, the state observer and, on
//! macOS arm64, the store-backed reads.
use hf_application::ctl::{
    state_inspect::{StorePolicy, inspect_state},
    task_reads::{ArtifactTargetPort, PendingLimits, Reads},
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
    /// A debug-only test seam: `HARFLOW_STATISTICS_BUDGET_MS` replaces the two-second budget so
    /// a test can exhaust it on a small store. A release build always uses the default.
    fn statistics_budget(&self) -> std::time::Duration {
        let default = hf_application::ctl::task_reads::STATISTICS_BUDGET;
        #[cfg(debug_assertions)]
        {
            std::env::var("HARFLOW_STATISTICS_BUDGET_MS")
                .ok()
                .and_then(|text| text.parse::<u64>().ok())
                .map_or(default, std::time::Duration::from_millis)
        }
        #[cfg(not(debug_assertions))]
        {
            default
        }
    }
    /// A debug-only test seam: `HARFLOW_STATISTICS_EXPIRE_AFTER_ROWS=N` counts the statistics
    /// budget as spent once N rows are read. It can only shorten the budget, and a release
    /// build never reads it.
    fn statistics_expire_after_rows(&self) -> Option<usize> {
        #[cfg(debug_assertions)]
        {
            std::env::var("HARFLOW_STATISTICS_EXPIRE_AFTER_ROWS")
                .ok()
                .and_then(|text| text.parse::<usize>().ok())
        }
        #[cfg(not(debug_assertions))]
        {
            None
        }
    }
    /// A debug-only test seam: `HARFLOW_PENDING_ENTRIES`, `HARFLOW_PENDING_NAME_BYTES` and
    /// `HARFLOW_PENDING_READ_BYTES` lower the namespace and read bounds so a test can reach
    /// them on a small store; they can never raise one. A release build always uses the contract's bounds.
    fn pending_limits(&self) -> PendingLimits {
        #[allow(unused_mut)]
        let mut limits = PendingLimits::default();
        #[cfg(debug_assertions)]
        {
            let number = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<u64>().ok());
            if let Some(value) = number("HARFLOW_PENDING_ENTRIES") {
                limits.entries = limits
                    .entries
                    .min(usize::try_from(value).unwrap_or(usize::MAX));
            }
            if let Some(value) = number("HARFLOW_PENDING_NAME_BYTES") {
                limits.name_bytes = limits
                    .name_bytes
                    .min(usize::try_from(value).unwrap_or(usize::MAX));
            }
            if let Some(value) = number("HARFLOW_PENDING_READ_BYTES") {
                limits.read_bytes = limits.read_bytes.min(value);
            }
        }
        limits
    }
    fn retention_days(&self) -> Option<String> {
        std::env::var_os("HARFLOW_RETENTION_DAYS").map(|value| value.to_string_lossy().into_owned())
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
