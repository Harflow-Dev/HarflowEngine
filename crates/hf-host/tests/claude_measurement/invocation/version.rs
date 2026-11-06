use super::{claude_fixture_command, request, runner};
use hf_application::owner_runtime::DigestPort;
use hf_domain::process::ProcessExit;
use hf_platform::qualification::FixtureRoot;
use runner::{run, run_until};
use std::{ffi::OsString, path::Path};

pub(in super::super) struct VersionObservation(runner::RunResult, bool);

impl VersionObservation {
    pub(in super::super) fn is_supported(&self) -> bool {
        self.1 && supported_version(&self.0)
    }

    pub(in super::super) fn require_supported(&self) {
        assert!(self.1, "binary bookend changed");
        require_version(&self.0);
    }
}

pub(in super::super) fn capture_version_only(
    root: &FixtureRoot,
    profile: &str,
    selection: &super::super::identity::VendorSelection,
    port: u16,
    nonce: &str,
) -> VersionObservation {
    let request = request(root, selection, port, nonce);
    let command = claude_fixture_command(&request).expect("isolated diagnostic command");
    assert_eq!(command.executable, request.logical_executable);
    let declared_args = ["--version".to_owned()];
    let config_digest = declared_config_digest(
        &command.executable,
        &declared_args,
        &command.env,
        command.source_profile,
        &root.workspace(),
    );
    eprintln!(
        "phase=claude-version declared-version-config-sha256={config_digest} scope=declared-config-only managed-external-settings=unknown"
    );
    let environment = command
        .env
        .iter()
        .map(|(key, value)| (key.as_str().into(), value.as_str().into()))
        .collect();
    VersionObservation(
        capture_version(root, profile, &command.executable, environment, selection),
        binary_matches(selection),
    )
}

pub(in crate::claude_measurement) fn capture_version_only_until(
    root: &FixtureRoot,
    profile: &str,
    selection: &super::super::identity::VendorSelection,
    port: u16,
    nonce: &str,
    until: std::time::Instant,
) -> VersionObservation {
    let request = request(root, selection, port, nonce);
    let command = claude_fixture_command(&request).expect("isolated diagnostic command");
    assert_eq!(command.executable, request.logical_executable);
    let environment = command
        .env
        .iter()
        .map(|(key, value)| (key.as_str().into(), value.as_str().into()))
        .collect();
    eprintln!("phase=claude-version begin exact-deadline={until:?}");
    let result = run_until(
        root,
        profile,
        &command.executable,
        &["--version".into()],
        environment,
        until,
        "claude-version",
    );
    VersionObservation(result, true)
}

pub(super) fn declared_config_digest(
    executable: &str,
    args: &[String],
    env: &[(String, String)],
    source_profile: &str,
    workspace_cwd: &Path,
) -> String {
    let mut bytes = Vec::new();
    field(&mut bytes, executable.as_bytes());
    count(&mut bytes, args.len());
    for argument in args {
        field(&mut bytes, argument.as_bytes());
    }
    count(&mut bytes, env.len());
    for (key, value) in env {
        field(&mut bytes, key.as_bytes());
        field(&mut bytes, value.as_bytes());
    }
    field(&mut bytes, source_profile.as_bytes());
    field(
        &mut bytes,
        workspace_cwd
            .to_str()
            .expect("declared workspace CWD is UTF-8")
            .as_bytes(),
    );
    hf_storage::Sha256Digest
        .digest_parts(&[&bytes])
        .as_str()
        .to_owned()
}

fn count(bytes: &mut Vec<u8>, value: usize) {
    bytes.extend_from_slice(&(value as u64).to_be_bytes());
}

fn field(bytes: &mut Vec<u8>, value: &[u8]) {
    count(bytes, value.len());
    bytes.extend_from_slice(value);
}

pub(super) fn observe_version(
    root: &FixtureRoot,
    profile: &str,
    executable: &str,
    env: Vec<(OsString, OsString)>,
    selection: &super::super::identity::VendorSelection,
) {
    let result = capture_version(root, profile, executable, env, selection);
    verify_binary_bookend(selection);
    require_version(&result);
}

fn capture_version(
    root: &FixtureRoot,
    profile: &str,
    executable: &str,
    env: Vec<(OsString, OsString)>,
    _selection: &super::super::identity::VendorSelection,
) -> runner::RunResult {
    eprintln!("phase=claude-version begin");
    let result = run(
        root,
        profile,
        executable,
        &["--version".into()],
        env,
        12_000,
        "claude-version",
    );
    eprintln!(
        "phase=claude-version complete exit={:?} child identity={}",
        result.exit, result.identity
    );
    result
}

fn supported_version(result: &runner::RunResult) -> bool {
    result.capture_is_complete()
        && result.exit == ProcessExit::Code(0)
        && result.stderr.is_empty()
        && String::from_utf8_lossy(&result.stdout).contains("2.1.318")
}

fn require_version(result: &runner::RunResult) {
    assert!(
        result.capture_is_complete(),
        "complete untruncated version capture"
    );
    assert_eq!(
        result.exit,
        ProcessExit::Code(0),
        "phase=claude-version child identity={}; unsupported version route; stdout={} stderr={}",
        result.identity,
        super::super::bounded_summary(&result.stdout),
        super::super::bounded_summary(&result.stderr)
    );
    assert!(
        result.stderr.is_empty(),
        "phase=claude-version stderr={}",
        super::super::bounded_summary(&result.stderr)
    );
    assert!(
        String::from_utf8_lossy(&result.stdout).contains("2.1.318"),
        "observed version differs from pinned 2.1.318"
    );
}

pub(super) fn binary_matches(selection: &super::super::identity::VendorSelection) -> bool {
    let current = super::super::identity::selection();
    current.logical == selection.logical && current.expected_hash == selection.expected_hash
}

pub(super) fn verify_binary_bookend(selection: &super::super::identity::VendorSelection) {
    let current = super::super::identity::selection();
    assert_eq!(
        current.logical, selection.logical,
        "logical selection stayed stable around measurement"
    );
    assert_eq!(
        current.expected_hash, selection.expected_hash,
        "physical executable bytes stayed stable around measurement"
    );
}

#[cfg(test)]
mod tests {
    use super::declared_config_digest;
    use std::path::Path;

    #[test]
    fn declared_config_digest_binds_ordered_arguments_and_environment_without_concat_collisions() {
        let args = vec!["--version".to_owned()];
        let env = vec![("A".to_owned(), "bc".to_owned())];
        let baseline =
            declared_config_digest("/claude", &args, &env, "profile", Path::new("/work"));
        assert_ne!(
            declared_config_digest(
                "/claude",
                &["--version".into(), "A".into()],
                &[],
                "profile",
                Path::new("/work")
            ),
            declared_config_digest(
                "/claude",
                &["--version".into()],
                &[("A".into(), String::new())],
                "profile",
                Path::new("/work")
            )
        );
        assert_ne!(
            baseline,
            declared_config_digest(
                "/claude",
                &["--version!".into()],
                &env,
                "profile",
                Path::new("/work")
            )
        );
        assert_ne!(
            baseline,
            declared_config_digest(
                "/claude",
                &args,
                &[("Ab".into(), "c".into())],
                "profile",
                Path::new("/work")
            )
        );
        assert_ne!(
            declared_config_digest(
                "/claude",
                &args,
                &[("A".into(), "b".into()), ("C".into(), "d".into())],
                "profile",
                Path::new("/work")
            ),
            declared_config_digest(
                "/claude",
                &args,
                &[("A".into(), "bC".into()), ("d".into(), String::new())],
                "profile",
                Path::new("/work")
            )
        );
    }
}
