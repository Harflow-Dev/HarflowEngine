use super::{claude_fixture_command, request, runner};
use hf_domain::process::ProcessExit;
use hf_platform::qualification::FixtureRoot;
use runner::run;
use std::ffi::OsString;

pub(in super::super) struct VersionObservation(runner::RunResult);

impl VersionObservation {
    pub(in super::super) fn require_supported(&self) {
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
    let environment = command
        .env
        .iter()
        .map(|(key, value)| (key.as_str().into(), value.as_str().into()))
        .collect();
    VersionObservation(capture_version(
        root,
        profile,
        &command.executable,
        environment,
        selection,
    ))
}

pub(super) fn observe_version(
    root: &FixtureRoot,
    profile: &str,
    executable: &str,
    env: Vec<(OsString, OsString)>,
    selection: &super::super::identity::VendorSelection,
) {
    let result = capture_version(root, profile, executable, env, selection);
    require_version(&result);
}

fn capture_version(
    root: &FixtureRoot,
    profile: &str,
    executable: &str,
    env: Vec<(OsString, OsString)>,
    selection: &super::super::identity::VendorSelection,
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
    verify_binary_bookend(selection);
    eprintln!(
        "phase=claude-version complete exit={:?} child identity={}",
        result.exit, result.identity
    );
    result
}

fn require_version(result: &runner::RunResult) {
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
