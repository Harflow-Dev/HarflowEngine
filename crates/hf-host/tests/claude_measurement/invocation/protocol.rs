use super::{claude_fixture_command, request, runner, version};
use hf_backends::claude_measurement::ClaudeFixtureCommand;
use hf_domain::process::ProcessExit;
use hf_platform::qualification::FixtureRoot;
use std::ffi::OsString;

pub(in super::super) struct ProtocolObservation {
    version: version::VersionObservation,
    fixture: Option<runner::RunResult>,
    binary_stable: bool,
}

impl ProtocolObservation {
    pub(in super::super) fn require_supported(&self, nonce: &str) {
        self.version.require_supported();
        assert!(self.binary_stable, "protocol binary bookend changed");
        let result = self
            .fixture
            .as_ref()
            .expect("protocol launched after valid version");
        assert!(
            result.capture_is_complete(),
            "complete untruncated protocol capture"
        );
        assert_eq!(result.exit, ProcessExit::Code(0), "protocol process exit");
        assert!(result.stderr.is_empty(), "protocol stderr must be empty");
        runner::require_protocol_witnesses(&result.stdout, nonce);
    }
}

pub(in super::super) fn capture_protocol(
    root: &FixtureRoot,
    profile: &str,
    selection: &super::super::identity::VendorSelection,
    port: u16,
    nonce: &str,
) -> ProtocolObservation {
    let version = version::capture_version_only(root, profile, selection, port, nonce);
    let fixture = if version.is_supported() {
        let request = request(root, selection, port, nonce);
        let command = with_bundled_timezone(
            claude_fixture_command(&request).expect("isolated scripted protocol command"),
        );
        assert_eq!(command.executable, request.logical_executable);
        let digest = version::declared_config_digest(
            &command.executable,
            &command.args,
            &command.env,
            command.source_profile,
            &root.workspace(),
        );
        eprintln!(
            "phase=claude-protocol declared-protocol-config-sha256={digest} scope=declared-config-only managed-external-settings=unknown"
        );
        let args = command.args.iter().map(OsString::from).collect::<Vec<_>>();
        let env = command
            .env
            .iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        Some(runner::run(
            root,
            profile,
            &command.executable,
            &args,
            env,
            90_000,
            "claude-protocol",
        ))
    } else {
        eprintln!("phase=claude-protocol skipped=unsupported-version");
        None
    };
    ProtocolObservation {
        version,
        fixture,
        binary_stable: version::binary_matches(selection),
    }
}

fn with_bundled_timezone(mut command: ClaudeFixtureCommand) -> ClaudeFixtureCommand {
    assert!(
        !command
            .env
            .iter()
            .any(|(key, _)| key == "ICU_TIMEZONE_FILES_DIR"),
        "protocol timezone configuration must be explicit and unique"
    );
    command
        .env
        .push(("ICU_TIMEZONE_FILES_DIR".into(), String::new()));
    eprintln!(
        "phase=claude-protocol config-delta=ICU_TIMEZONE_FILES_DIR-empty source=bundled-public-data"
    );
    command
}

#[cfg(test)]
#[path = "protocol/environment_tests.rs"]
mod environment_tests;
