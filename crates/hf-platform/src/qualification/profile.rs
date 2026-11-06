use super::fixture::FixtureRoot;
use crate::process::SpawnSpec;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfilePolicy {
    ControlledHelper,
    VendorNoFork,
    /// Negative evidence only: permits a deliberately seeded alias topology.
    AliasCounterexample,
}
pub fn quote_sbpl(value: &str) -> io::Result<String> {
    if value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid SBPL string",
        ));
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}
fn quoted_path(path: &Path) -> io::Result<String> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "absolute profile path required",
        ));
    }
    quote_sbpl(
        path.to_str()
            .ok_or_else(|| io::Error::other("non UTF-8 profile path"))?,
    )
}
impl FixtureRoot {
    pub fn profile(
        &self,
        policy: ProfilePolicy,
        selected_read_paths: &[PathBuf],
        port: Option<u16>,
    ) -> io::Result<String> {
        if selected_read_paths.len() > 16 || port == Some(0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "profile input budget or port",
            ));
        }
        if policy == ProfilePolicy::AliasCounterexample {
            self.validate_root()?;
        } else {
            self.validate_topology()?;
        }
        render_profile(self.root(), policy, selected_read_paths, port)
    }
    /// Fixed physical wrapper; only env changes the child's cwd. Selected logical
    /// executable remains untouched, including symlink launch semantics.
    pub fn spawn_spec(
        &self,
        profile: &str,
        selected: &Path,
        args: &[OsString],
        environment: Vec<(OsString, OsString)>,
        capture_limit: usize,
    ) -> io::Result<SpawnSpec> {
        self.validate_root()?;
        if !selected.is_absolute()
            || args.len() > 110
            || profile.len() > 32768
            || capture_limit > 1_048_576
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "measurement spawn budget",
            ));
        }
        let mut wrapped: Vec<OsString> = ["-p", profile, "/usr/bin/env", "-C"]
            .into_iter()
            .map(OsString::from)
            .collect();
        wrapped.push(self.workspace().into_os_string());
        wrapped.push(selected.as_os_str().to_owned());
        wrapped.extend_from_slice(args);
        Ok(SpawnSpec {
            executable: PathBuf::from("/usr/bin/sandbox-exec"),
            cwd: std::env::current_dir()?,
            args: wrapped,
            environment,
            capture_limit,
        })
    }
}

/// Shared byte renderer; topology authority stays with its caller.
pub(super) fn render_profile(
    root: &Path,
    policy: ProfilePolicy,
    selected_read_paths: &[PathBuf],
    port: Option<u16>,
) -> io::Result<String> {
    let mut profile = String::from(
        "(version 1)\n(deny default)\n(allow process-exec)\n(allow file-read-metadata)\n",
    );
    // Apple dyld-support.sb documents bootstrap opening the root directory.
    // Literal root access does not grant reads beneath it.
    profile.push_str("(allow file-read-data (literal \"/\"))\n");
    // Exact public entropy device required by the measured Python helper.
    profile.push_str("(allow file-read-data (literal \"/dev/urandom\"))\n");
    for path in ["/System", "/usr/lib", "/usr/share", "/Library/Apple/System"] {
        profile.push_str(&format!(
            "(allow file-read-data (subpath {}))\n",
            quote_sbpl(path)?
        ));
    }
    profile.push_str(&format!(
        "(allow file-read-data (subpath {}))\n",
        quoted_path(root)?
    ));
    for path in selected_read_paths.iter().chain(
        [
            PathBuf::from("/usr/bin/sandbox-exec"),
            PathBuf::from("/usr/bin/env"),
        ]
        .iter(),
    ) {
        let physical = std::fs::canonicalize(path)?;
        let meta = std::fs::metadata(&physical)?;
        let filter = if meta.is_file() {
            "literal"
        } else if meta.is_dir()
            && (physical.starts_with("/Library/Developer")
                || physical.starts_with("/Applications/Xcode.app/Contents/Developer"))
        {
            "subpath"
        } else {
            return Err(io::Error::other(
                "read closure must be exact file or system developer runtime",
            ));
        };
        profile.push_str(&format!(
            "(allow file-read-data ({filter} {}))\n",
            quoted_path(&physical)?
        ));
    }
    write_rules(root, policy, &mut profile)?;
    profile.push_str("(deny file-link signal mach-lookup appleevent-send network*)\n");
    profile.push_str("(deny file-read-data (regex #\"^/Users/[^/]+/Library/Keychains(/|$)\") (regex #\"^/Users/[^/]+/[.]harflow(/|$)\"))\n");
    // SBPL localhost scopes this port on both IPv4 and IPv6 loopback.
    // The measurement harness must own both endpoints before vendor startup.
    if let Some(port) = port {
        profile.push_str(&format!(
            "(allow network-outbound (remote ip \"localhost:{port}\"))\n"
        ));
    }
    if policy == ProfilePolicy::VendorNoFork {
        profile.push_str("(deny process-fork)\n");
    } else {
        profile.push_str("(allow process-fork)\n");
    }
    Ok(profile)
}
fn write_rules(root: &Path, policy: ProfilePolicy, profile: &mut String) -> io::Result<()> {
    for path in [root.join("workspace"), root.join("runtime")] {
        profile.push_str(&format!(
            "(allow file-write* (subpath {}))\n(deny file-write-unlink (literal {}))\n",
            quoted_path(&path)?,
            quoted_path(&path)?
        ));
    }
    profile.push_str(&format!(
        "(deny file-write* (subpath {}))\n(deny file-write-unlink (literal {}))\n",
        quoted_path(&root.join("workspace/nested/control"))?,
        quoted_path(root)?
    ));
    profile.push_str(&format!(
        "(deny file-write* (subpath {}))\n",
        quoted_path(&root.join("workspace").join(".git"))?
    ));
    if policy != ProfilePolicy::AliasCounterexample {
        profile.push_str(&format!(
            "(deny file-write-unlink (literal {}))\n",
            quoted_path(&root.join("workspace").join("nested"))?
        ));
    }
    Ok(())
}
