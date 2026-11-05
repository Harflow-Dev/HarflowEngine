#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_backends::claude_measurement::{ClaudeFixtureRequest, claude_fixture_command};
use hf_domain::process::ProcessExit;
use hf_platform::qualification::FixtureRoot;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

#[path = "runner.rs"]
mod runner;
use runner::{require_protocol_witnesses, run};
#[path = "invocation/version.rs"]
mod version;
pub(super) use version::{VersionObservation, capture_version_only};
use version::{observe_version, verify_binary_bookend};

pub(super) fn launch_vendor(
    root: &FixtureRoot,
    profile: &str,
    selection: &super::identity::VendorSelection,
    port: u16,
    nonce: &str,
) {
    let request = request(root, selection, port, nonce);
    let command = claude_fixture_command(&request).expect("isolated fixture command");
    assert_eq!(
        command.executable, request.logical_executable,
        "preserve selected logical path"
    );
    assert!(command.source_profile.contains("no-fork"));
    assert!(
        !command
            .env
            .iter()
            .any(|(key, _)| key.to_ascii_uppercase().contains("PROXY"))
    );
    let environment: Vec<(OsString, OsString)> = command
        .env
        .iter()
        .map(|(key, value)| (key.as_str().into(), value.as_str().into()))
        .collect();
    observe_version(
        root,
        profile,
        &command.executable,
        environment.clone(),
        selection,
    );
    observe_fixture(
        root,
        profile,
        &command.executable,
        &command.args,
        environment,
        nonce,
    );
    verify_binary_bookend(selection);
}

pub(super) fn launch_version_only(
    root: &FixtureRoot,
    profile: &str,
    selection: &super::identity::VendorSelection,
    port: u16,
    nonce: &str,
) {
    let request = request(root, selection, port, nonce);
    let command = claude_fixture_command(&request).expect("isolated diagnostic command");
    assert_eq!(command.executable, request.logical_executable);
    let environment = command
        .env
        .iter()
        .map(|(key, value)| (key.as_str().into(), value.as_str().into()))
        .collect();
    observe_version(root, profile, &command.executable, environment, selection);
    verify_binary_bookend(selection);
}

fn observe_fixture(
    root: &FixtureRoot,
    profile: &str,
    executable: &str,
    args: &[String],
    env: Vec<(OsString, OsString)>,
    nonce: &str,
) {
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    eprintln!("phase=claude-fixture begin");
    let result = run(
        root,
        profile,
        executable,
        &args,
        env,
        90_000,
        "claude-fixture",
    );
    eprintln!(
        "phase=claude-fixture complete exit={:?} child identity={}",
        result.exit, result.identity
    );
    assert_eq!(
        result.exit,
        ProcessExit::Code(0),
        "phase=claude-fixture child identity={}; unsupported fixture route; stdout={} stderr={}",
        result.identity,
        super::bounded_summary(&result.stdout),
        super::bounded_summary(&result.stderr)
    );
    assert!(
        result.stderr.is_empty(),
        "phase=claude-fixture stderr={}",
        super::bounded_summary(&result.stderr)
    );
    require_protocol_witnesses(&result.stdout, nonce);
}

pub(super) fn prepare_private_dirs(root: &FixtureRoot) {
    let private = root.runtime().join("claude-private");
    fs::create_dir(&private).expect("fresh Claude runtime root");
    for path in private_paths(root) {
        fs::create_dir(path).expect("fresh private Claude directory");
    }
}

pub(super) fn preflight_environment(root: &FixtureRoot) -> Vec<(OsString, OsString)> {
    let paths = private_paths(root);
    [
        ("HOME", paths[0].as_path()),
        ("TMPDIR", paths[4].as_path()),
        ("PATH", Path::new("/usr/bin:/bin:/usr/sbin:/sbin")),
        ("LANG", Path::new("C")),
        ("PYTHONNOUSERSITE", Path::new("1")),
        ("PYTHONDONTWRITEBYTECODE", Path::new("1")),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.as_os_str().to_owned()))
    .collect()
}

fn private_paths(root: &FixtureRoot) -> [PathBuf; 6] {
    [
        "home",
        "config",
        "xdg-config",
        "xdg-cache",
        "tmp",
        "claude-tmp",
    ]
    .map(|name| root.runtime().join("claude-private").join(name))
}

fn request(
    root: &FixtureRoot,
    selection: &super::identity::VendorSelection,
    port: u16,
    nonce: &str,
) -> ClaudeFixtureRequest {
    let paths = private_paths(root);
    let strings = paths.map(|path| path.to_str().expect("fixture paths are UTF-8").to_owned());
    ClaudeFixtureRequest {
        logical_executable: selection
            .logical
            .to_str()
            .expect("logical Claude selection is UTF-8")
            .to_owned(),
        home: strings[0].clone(),
        config: strings[1].clone(),
        xdg_config: strings[2].clone(),
        xdg_cache: strings[3].clone(),
        temporary: strings[4].clone(),
        claude_temporary: strings[5].clone(),
        port,
        nonce: nonce.into(),
    }
}
