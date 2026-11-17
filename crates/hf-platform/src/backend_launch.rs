//! Normal PATH discovery with a separately retained physical executable identity.
mod identity;
mod version;

pub use version::prepare_version_probe;

use crate::process::RetainedOwnedChild;
use hf_application::{prepared_launch::StreamingDigestFactory, task_launch::ClaudeDiscoveryPort};
use hf_domain::task_launch::{DiscoveredClaude, LaunchRefusal};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Instant,
};

/// Observe a physical executable without invoking it; used by private bootstrap qualification.
pub fn observe_binary_pin(
    physical: &Path,
    hash: &impl StreamingDigestFactory,
    deadline: Instant,
) -> Result<hf_domain::prepared_launch::BinaryPin, LaunchRefusal> {
    identity::observe(physical, hash, deadline)
}

/// Composition must retain this adapter or transfer its pending probe to the owner
/// on error. Dropping it is not a child-settlement operation.
pub struct PathClaudeDiscovery<H> {
    hash: H,
    environment: Vec<(OsString, OsString)>,
    pending: Option<RetainedOwnedChild>,
}
impl<H> PathClaudeDiscovery<H> {
    pub fn new(hash: H, environment: Vec<(OsString, OsString)>) -> Self {
        Self {
            hash,
            environment,
            pending: None,
        }
    }
    pub fn pending_probe(&mut self) -> Option<&mut RetainedOwnedChild> {
        self.pending.as_mut()
    }
    /// Transfers the original waiter, pipes and signal authority, never a PID-only handle.
    pub fn take_pending_probe(&mut self) -> Option<RetainedOwnedChild> {
        self.pending.take()
    }
}
impl<H: StreamingDigestFactory> ClaudeDiscoveryPort for PathClaudeDiscovery<H> {
    fn discover(&mut self, deadline: Instant) -> Result<DiscoveredClaude, LaunchRefusal> {
        if self.pending.is_some() {
            return Err(LaunchRefusal::ProbeUnsettled);
        }
        timely(deadline)?;
        let logical = path_candidate(&self.environment)?;
        let physical = std::fs::canonicalize(&logical).map_err(|_| LaunchRefusal::Unavailable)?;
        let mut binary = identity::observe(&physical, &self.hash, deadline)?;
        binary.logical_path = text_path(&logical)?;
        let version = version::probe(
            &binary,
            &self.environment,
            &self.hash,
            &mut self.pending,
            deadline,
        )?;
        if std::fs::canonicalize(&logical).ok().as_ref() != Some(&physical)
            || identity::observe(&physical, &self.hash, deadline)? != {
                let mut pin = binary.clone();
                pin.logical_path = pin.physical_path.clone();
                pin
            }
        {
            return Err(LaunchRefusal::IdentityChanged);
        }
        timely(deadline)?;
        Ok(DiscoveredClaude { binary, version })
    }
}
fn path_candidate(environment: &[(OsString, OsString)]) -> Result<PathBuf, LaunchRefusal> {
    if environment.len() > 128 {
        return Err(LaunchRefusal::InvalidDiscovery);
    }
    let paths: Vec<_> = environment
        .iter()
        .filter(|(key, _)| key == "PATH")
        .collect();
    if paths.len() != 1 || paths[0].1.len() > 16384 {
        return Err(LaunchRefusal::InvalidDiscovery);
    }
    for (count, directory) in std::env::split_paths(&paths[0].1).enumerate() {
        if count >= 128 || !directory.is_absolute() {
            return Err(LaunchRefusal::InvalidDiscovery);
        }
        text_path(&directory)?;
        let candidate = directory.join("claude");
        match std::fs::metadata(&candidate) {
            Ok(metadata) => {
                use std::os::unix::fs::PermissionsExt;
                if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
                    return Ok(candidate);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(LaunchRefusal::Unavailable),
        }
    }
    Err(LaunchRefusal::NotFound)
}
fn text_path(path: &Path) -> Result<String, LaunchRefusal> {
    let value = path.to_str().ok_or(LaunchRefusal::InvalidDiscovery)?;
    if !path.is_absolute()
        || value.len() > 4096
        || value.chars().any(char::is_control)
        || value.split('/').any(|part| part == "." || part == "..")
    {
        return Err(LaunchRefusal::InvalidDiscovery);
    }
    Ok(value.into())
}
fn timely(deadline: Instant) -> Result<(), LaunchRefusal> {
    if Instant::now() >= deadline {
        Err(LaunchRefusal::Deadline)
    } else {
        Ok(())
    }
}
