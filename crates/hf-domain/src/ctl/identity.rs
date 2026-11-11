//! Public wire identity constants and host naming.
pub const PRODUCT: &str = "harflow";
pub const PRODUCT_VERSION: &str = "0.0.13+rust";
pub const WIRE_VERSION: u32 = 1;
pub const CTL_FORMAT: &str = "harflow.ctl";
pub const SETUP_FORMAT: &str = "harflow.setup";
pub const EVENT_FORMAT: &str = "harflow.event";
pub const VERSION_INFO_FORMAT: &str = "harflow.version-info";
pub const PROGRAM: &str = "harflow-ctl";
/// One advertised capability. A store-gated capability is advertised only where the
/// store-backed reads exist (macOS arm64), after the paths it qualifies pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capability {
    pub name: &'static str,
    pub store_gated: bool,
}
const fn always(name: &'static str) -> Capability {
    Capability {
        name,
        store_gated: false,
    }
}
const fn gated(name: &'static str) -> Capability {
    Capability {
        name,
        store_gated: true,
    }
}
/// Advertised capabilities in the order of the canonical Python list (`wire_contracts.py`
/// `CAPABILITIES`). The list grows only when a path and its evidence land: the pending
/// launch capability is added with its commands.
pub const CAPABILITIES: [Capability; 9] = [
    gated("interaction-events.v1"),
    always("ctl.v1"),
    always("version-info.v1"),
    gated("statistics-page.v1"),
    gated("task-event-page.v1"),
    gated("task-prompt.v1"),
    gated("task-successors.v1"),
    gated("task-artifacts.v1"),
    gated("task-continuation-stop.v1"),
];
/// The names a handshake lists, in canonical order, with the store-gated ones only when
/// the store-backed reads exist.
pub fn capabilities(store_reads: bool) -> Vec<&'static str> {
    CAPABILITIES
        .iter()
        .filter(|capability| store_reads || !capability.store_gated)
        .map(|capability| capability.name)
        .collect()
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostIdentity {
    pub os: String,
    pub architecture: String,
}
impl HostIdentity {
    /// Maps Rust target names to the names the public contract uses.
    pub fn from_rust(os: &str, architecture: &str) -> Self {
        let name = match os {
            "macos" => "darwin",
            "windows" => "win32",
            other => other,
        };
        let machine = match (os, architecture) {
            ("macos", "aarch64") => "arm64",
            ("windows", "x86_64") => "AMD64",
            ("windows", "aarch64") => "ARM64",
            (_, other) => other,
        };
        Self {
            os: name.to_string(),
            architecture: machine.to_string(),
        }
    }
}
