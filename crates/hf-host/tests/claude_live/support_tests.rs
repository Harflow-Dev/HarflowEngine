#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;
use hf_backends::{claude_live_probe::live_command, claude_measurement::ClaudeFixtureRequest};
use hf_domain::process::ProcessExit;
use hf_platform::process::{
    MacIdentity, RetainedOwnedChild, SpawnInitialization, SpawnSpec, WaitOutcome,
};
use std::{
    collections::BTreeMap,
    env, fs,
    path::PathBuf,
    time::{Duration, Instant},
};

const TEST_TOKEN: &str = "offline-live-environment-probe";

fn command() -> hf_backends::claude_measurement::ClaudeFixtureCommand {
    let request = ClaudeFixtureRequest {
        logical_executable: "/opt/homebrew/bin/claude".into(),
        home: "/tmp/owned/home".into(),
        config: "/tmp/owned/config".into(),
        xdg_config: "/tmp/owned/xdg-config".into(),
        xdg_cache: "/tmp/owned/xdg-cache".into(),
        temporary: "/tmp/owned/tmp".into(),
        claude_temporary: "/tmp/owned/claude-tmp".into(),
        port: 43210,
        nonce: "a1b2c3d4e5f60718".into(),
    };
    live_command(&request, request.port).unwrap()
}

fn live_test_environment() -> Vec<(OsString, OsString)> {
    let mut pairs = command().env;
    pairs.push(("CLAUDE_CODE_OAUTH_TOKEN".into(), TEST_TOKEN.into()));
    environment(&pairs)
}

fn assert_unique_and_preserved(environment: &[(OsString, OsString)]) {
    let mut observed = BTreeMap::new();
    for (key, value) in environment {
        let key = key.to_string_lossy().into_owned();
        assert!(
            observed
                .insert(key, value.to_string_lossy().into_owned())
                .is_none(),
            "helper environment contains duplicate names"
        );
    }
    for (key, value) in [
        ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin"),
        ("LANG", "C"),
        ("LC_ALL", "C"),
        ("ICU_TIMEZONE_FILES_DIR", ""),
        ("CLAUDE_CODE_OAUTH_TOKEN", TEST_TOKEN),
    ] {
        assert!(
            observed.get(key).is_some_and(|actual| actual == value),
            "explicit helper environment value was not preserved"
        );
    }
}

#[test]
fn live_helper_environment_is_unique_and_preserves_explicit_values() {
    assert_unique_and_preserved(&live_test_environment());
}

#[test]
fn fixed_native_true_accepts_the_live_helper_environment_and_settles_cleanly() {
    let current = fs::canonicalize(env::current_dir().unwrap()).unwrap();
    let child_env = live_test_environment();
    assert_unique_and_preserved(&child_env);
    let spec = SpawnSpec {
        executable: PathBuf::from("/usr/bin/true"),
        cwd: current,
        args: Vec::new(),
        environment: child_env,
        capture_limit: 4096,
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut child = RetainedOwnedChild::spawn(spec, &MacIdentity, deadline).unwrap();
    assert!(
        child.initialization() == SpawnInitialization::Ready,
        "fixed native child initialization refused"
    );
    child.close_stdin();
    let wait = child.wait_until(deadline, || false).unwrap();
    assert!(
        wait == WaitOutcome::Exited(ProcessExit::Code(0)),
        "fixed native child did not exit successfully"
    );
    while (!child.capture().stdout.eof || !child.capture().stderr.eof) && Instant::now() < deadline
    {
        child.pump(deadline, 4096).unwrap();
    }
    let capture = child.capture();
    assert!(
        capture.stdout.eof && capture.stderr.eof,
        "fixed native child pipes did not reach EOF"
    );
    assert!(
        !capture.stdout.overflow && !capture.stderr.overflow,
        "fixed native child capture overflowed"
    );
    assert!(
        capture.stdout.error.is_none() && capture.stderr.error.is_none(),
        "fixed native child capture failed"
    );
    assert!(
        capture.stdout.bytes.is_empty() && capture.stderr.bytes.is_empty(),
        "fixed native child emitted unexpected output"
    );
    assert!(
        child.exit() == ProcessExit::Code(0),
        "original wait did not retain the successful exit"
    );
}
