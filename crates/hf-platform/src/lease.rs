//! Process-wide retained sentinel descriptors. No descriptor eviction is permitted.
use hf_application::lease::{LeaseError, LeaseGuard, LeaseMode, LeasePort};
use std::{
    path::Path,
    sync::{Arc, OnceLock},
    time::Instant,
};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod darwin;
pub fn shared() -> Arc<dyn LeasePort> {
    static INSTANCE: OnceLock<Arc<Adapter>> = OnceLock::new();
    INSTANCE.get_or_init(|| Arc::new(Adapter)).clone()
}
struct Adapter;
impl LeasePort for Adapter {
    fn acquire(
        &self,
        path: &Path,
        mode: LeaseMode,
        create: bool,
        deadline: Instant,
    ) -> Result<Box<dyn LeaseGuard>, LeaseError> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            darwin::acquire(path, mode, create, deadline)
        }
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            let _ = (path, mode, create, deadline);
            Err(LeaseError::Unavailable)
        }
    }
}
