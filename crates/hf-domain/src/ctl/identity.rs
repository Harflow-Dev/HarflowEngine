//! Public wire identity constants and host naming.
pub const PRODUCT: &str = "harflow";
pub const PRODUCT_VERSION: &str = "0.0.11+rust";
pub const WIRE_VERSION: u32 = 1;
pub const CTL_FORMAT: &str = "harflow.ctl";
pub const SETUP_FORMAT: &str = "harflow.setup";
pub const EVENT_FORMAT: &str = "harflow.event";
pub const VERSION_INFO_FORMAT: &str = "harflow.version-info";
pub const PROGRAM: &str = "harflow-ctl";
/// Advertised capabilities. The list grows only when a path and its evidence land.
pub const CAPABILITIES: [&str; 2] = ["ctl.v1", "version-info.v1"];
/// Advertised only where the store-backed reads exist (macOS arm64), after the paths they
/// qualify pass. The rest of the read surface sits under `ctl.v1`.
pub const STORE_READ_CAPABILITIES: [&str; 3] =
    ["task-successors.v1", "task-event-page.v1", "task-prompt.v1"];
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
