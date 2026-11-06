#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "claude_live/actors.rs"]
mod actors;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "offline local Node observer contract fixtures; no vendor or provider runtime"]
fn offline_local_node_contract_fixtures_settle_cleanly() {
    run_fixtures();
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn run_fixtures() {
    use hf_domain::process::ProcessExit;
    use hf_platform::{
        process::{MacIdentity, RetainedOwnedChild, SpawnSpec},
        qualification::FixtureRoot,
    };
    use std::{
        ffi::OsString,
        fs,
        path::PathBuf,
        time::{Duration, Instant},
    };

    const NODE: &str = "/opt/homebrew/bin/node";
    const FIXTURES: &[&str] = &[
        "claude_two_tunnel/client_reason_contract_test.cjs",
        "claude_two_tunnel/client_reason_writer_test.cjs",
    ];
    let root = FixtureRoot::create().expect("fresh offline fixture root");
    let root_path = root.root().to_path_buf();
    let node = fs::canonicalize(NODE).expect("preinstalled local Node");
    assert!(fs::metadata(&node).expect("local Node metadata").is_file());
    let cwd = std::env::current_dir().expect("controller cwd");
    let home = root.runtime().join("home");
    let tmp = root.runtime().join("tmp");
    fs::create_dir(&home).expect("fresh fixture HOME");
    fs::create_dir(&tmp).expect("fresh fixture TMPDIR");
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    for fixture in FIXTURES {
        let script = base.join(fixture);
        assert!(script.starts_with(&base) && script.is_file());
        let spec = SpawnSpec {
            executable: node.clone(),
            cwd: cwd.clone(),
            args: vec![OsString::from(script)],
            environment: vec![
                (OsString::from("HOME"), home.as_os_str().to_owned()),
                (OsString::from("TMPDIR"), tmp.as_os_str().to_owned()),
            ],
            capture_limit: 512,
        };
        let until = Instant::now() + Duration::from_secs(5);
        let child = RetainedOwnedChild::spawn(spec, &MacIdentity, until)
            .expect("retained original local Node child");
        let captured = actors::Original::new(child).collect(until);
        let identity = captured
            .original_identity
            .as_ref()
            .expect("typed original process identity");
        assert!(identity.pid() > 0);
        assert!(
            captured.complete,
            "Node fixture must settle within five seconds"
        );
        assert_eq!(captured.exit, ProcessExit::Code(0));
        assert!(captured.stdout.is_empty() && captured.stderr.is_empty());
        assert!(captured.stdout.len() <= 512 && captured.stderr.len() <= 512);
        assert!(captured.status.stdout_eof && captured.status.stderr_eof);
        assert!(!captured.status.deadline_expired);
        assert!(!captured.status.stdout_overflow && !captured.status.stderr_overflow);
        assert!(!captured.status.stdout_error_present && !captured.status.stderr_error_present);
        assert!(home.is_dir() && tmp.is_dir());
    }
    root.validate_topology().expect("offline fixture topology");
    drop(root);
    fs::remove_dir_all(root_path).expect("remove successful offline fixture root");
}
