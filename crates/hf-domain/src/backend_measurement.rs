//! Diagnostic observations only. These values confer no launch or admission authority.
use std::string::String;
use std::vec::Vec;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathIdentity {
    pub logical_path: String,
    pub physical_path: String,
    pub identity: String,
}

/// Exact scope of one observation, never a reusable platform qualification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeasurementBinding {
    pub backend: String,
    pub backend_version: String,
    pub logical_executable: String,
    pub physical_executable: String,
    pub executable_hash: String,
    pub runtime_closure: String,
    pub platform: String,
    pub sandbox_hash: String,
    pub wrapper_hash: String,
    pub profile_digest: String,
    pub configuration_digest: String,
    pub probe_revision: String,
    pub grant_digest: String,
    pub topology_provenance: String,
    pub paths: Vec<PathIdentity>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MeasurementOperation {
    Create,
    Truncate,
    Append,
    Unlink,
    Rename,
    Chmod,
    Symlink,
    ExistingHardlink,
    NewHardlink,
    ProtectedAncestorRename,
    InheritedDescriptor,
    SafeSpawnDescriptorExclusion,
    Fork,
    PosixSpawn,
    Descendant,
    NestedSandbox,
    ReadBoundary,
    ParentControl,
    Delegation,
    NetworkAllowedEndpoint,
    NetworkDeniedEndpoint,
    ClaudeNoToolsProtocol,
}

/// Observed operation behavior; none of these variants grants production authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationObservation {
    Allowed,
    Denied,
    Counterexample,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticMeasurement {
    pub binding: MeasurementBinding,
    pub operation: MeasurementOperation,
    pub observation: OperationObservation,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StrictRouteUnsupported {
    pub backend: String,
    pub reason: String,
}
