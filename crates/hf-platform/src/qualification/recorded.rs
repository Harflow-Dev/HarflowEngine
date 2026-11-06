//! Deny-only revalidation. Recorded identities never construct FixtureRoot authority.
use super::{
    fixture::{FixtureRoot, PhysicalIdentity, inventory, validate_directory},
    profile::{ProfilePolicy, render_profile},
};
use hf_domain::{
    capture::FileIdentity,
    claude_qualification::{RecordedDirectory, RecordedFixtureTopology, VENDOR_PATH},
};
use std::{
    io,
    path::{Path, PathBuf},
};
impl FixtureRoot {
    pub fn recorded_topology(&self) -> io::Result<RecordedFixtureTopology> {
        self.validate_topology()?;
        Ok(RecordedFixtureTopology {
            root: record(self.root(), self.identity()),
            roles: self
                .roles
                .iter()
                .map(|(path, identity)| record(path, identity))
                .collect(),
        })
    }
}
fn record(path: &Path, identity: &PhysicalIdentity) -> RecordedDirectory {
    RecordedDirectory {
        path: path.to_string_lossy().into_owned(),
        identity: FileIdentity {
            device: identity.device,
            inode: identity.inode,
        },
    }
}
pub fn validate_recorded_topology(value: &RecordedFixtureTopology) -> io::Result<()> {
    let root = Path::new(&value.root.path);
    let expected = [
        "workspace",
        "runtime",
        "workspace/nested",
        "workspace/.git",
        "workspace/nested/control",
        "outside",
    ];
    if !root.is_absolute() || value.roles.len() != expected.len() {
        return Err(io::Error::other("recorded topology shape"));
    }
    check(&value.root)?;
    for (role, suffix) in value.roles.iter().zip(expected) {
        if Path::new(&role.path) != root.join(suffix) {
            return Err(io::Error::other("recorded topology role"));
        }
        check(role)?;
    }
    inventory(root, 0, &mut 1024)?;
    for role in &value.roles {
        check(role)?;
    }
    check(&value.root)
}
fn check(value: &RecordedDirectory) -> io::Result<()> {
    validate_directory(
        Path::new(&value.path),
        &PhysicalIdentity {
            device: value.identity.device,
            inode: value.identity.inode,
        },
    )
}
/// Exact pre-existing measurement read closure; no gate-helper read exception.
pub fn qualification_read_paths() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Library/Developer/CommandLineTools/usr/bin/python3"),
        PathBuf::from("/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework"),
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../hf-host/tests/claude_measurement/preflight.py"
        )),
        PathBuf::from("/usr/bin/true"),
        PathBuf::from(VENDOR_PATH),
    ]
}
pub fn recorded_vendor_profile(value: &RecordedFixtureTopology, port: u16) -> io::Result<String> {
    if port == 0 {
        return Err(io::Error::other("recorded profile port"));
    }
    validate_recorded_topology(value)?;
    let mut profile = render_profile(
        Path::new(&value.root.path),
        ProfilePolicy::VendorNoFork,
        &qualification_read_paths(),
        Some(port),
    )?;
    profile.push_str("(allow sysctl-read (sysctl-name \"hw.pagesize_compat\"))\n");
    Ok(profile)
}
