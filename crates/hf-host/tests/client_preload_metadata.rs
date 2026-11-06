#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_domain::process::ProcessIdentity;

#[path = "claude_two_tunnel/client_preload_metadata.rs"]
mod client_preload_metadata;

const NONCE: &str = "0123456789abcdef0123456789abcdef";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "claude_live/actors.rs"]
mod reporter_actors;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const REPORTER_SOURCE: &[u8] = include_bytes!("claude_two_tunnel/preload_probe.cjs");

fn identity(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 1, 0, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}

fn ready(nonce: &str, pid: u32) -> Vec<u8> {
    format!("CLIENT_PRELOAD_READY_V1\t{nonce}\t{pid}\n").into_bytes()
}

#[test]
fn accepts_only_exact_ready_marker_for_retained_process_identity() {
    let process = identity(42);
    assert_eq!(
        client_preload_metadata::parse_ready(&ready(NONCE, 42), NONCE, &process),
        Ok(())
    );

    let wrong_nonce = ready("fedcba9876543210fedcba9876543210", 42);
    let wrong_pid = ready(NONCE, 43);
    let unknown_version = b"CLIENT_PRELOAD_READY_V2\t0123456789abcdef0123456789abcdef\t42\n";
    let duplicate = [ready(NONCE, 42), ready(NONCE, 42)].concat();
    let trailing_field = b"CLIENT_PRELOAD_READY_V1\t0123456789abcdef0123456789abcdef\t42\textra\n";
    let torn = b"CLIENT_PRELOAD_READY_V1\t0123456789abcdef0123456789abcdef\t42";
    let malformed = b"CLIENT_PRELOAD_READY_V1\tnot-a-nonce\t42\n";
    let oversized = vec![b'x'; 513];
    for bytes in [
        wrong_nonce.as_slice(),
        wrong_pid.as_slice(),
        unknown_version.as_slice(),
        duplicate.as_slice(),
        trailing_field.as_slice(),
        torn.as_slice(),
        malformed.as_slice(),
        oversized.as_slice(),
    ] {
        let error = client_preload_metadata::parse_ready(bytes, NONCE, &process).unwrap_err();
        assert_eq!(error, "unavailable");
        assert!(!error.contains("CLIENT_PRELOAD"));
        assert!(!error.contains(NONCE));
    }
    assert_eq!(
        client_preload_metadata::parse_ready(&ready(NONCE, 42), "short", &process),
        Err("unavailable")
    );
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "offline local Node reporter contract fixture; no vendor or provider runtime"]
fn fixed_reporter_writes_marker_bound_to_original_node_process() {
    local_reporter::run();
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
mod local_reporter {
    use super::{REPORTER_SOURCE, client_preload_metadata, reporter_actors as actors};
    use hf_domain::process::ProcessExit;
    use hf_platform::qualification::FixtureRoot;
    use hf_platform::{
        process::{MacIdentity, RetainedOwnedChild, SpawnSpec},
        qualification::live::ObserverFile,
    };
    use std::{
        ffi::OsString,
        fs::{self, OpenOptions},
        io::Write,
        os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        path::Path,
        time::{Duration, Instant},
    };

    const NODE: &str = "/opt/homebrew/bin/node";

    pub(super) fn run() {
        let root = FixtureRoot::create().expect("fresh offline reporter fixture");
        let nonce = root
            .root()
            .file_name()
            .and_then(|value| value.to_str())
            .and_then(|value| value.strip_prefix("harflow-os-measurement-"))
            .expect("fixture root nonce")
            .to_owned();
        let (source, source_stamp) = write_source(&root);
        let node = fs::canonicalize(NODE).expect("installed local Node path");
        assert!(
            fs::metadata(&node)
                .expect("canonical Node metadata")
                .is_file()
        );
        let metadata_path = root.runtime().join("client-preload-ready");
        let metadata = ObserverFile::create(&metadata_path).expect("private metadata file");
        let capture = launch(&root, &node, &source, &metadata_path, &metadata, &nonce);

        assert!(
            capture.complete,
            "original Node child must settle cleanly: status={:?} exit={:?} stdout_bytes={} stderr_bytes={}",
            capture.status,
            capture.exit,
            capture.stdout.len(),
            capture.stderr.len()
        );
        assert_eq!(capture.exit, ProcessExit::Code(0));
        assert!(capture.stdout.is_empty() && capture.stderr.is_empty());
        assert!(capture.stdout.len() <= 512 && capture.stderr.len() <= 512);
        let identity = capture
            .original_identity
            .as_ref()
            .expect("typed original identity");
        let bytes = metadata.read_bounded().expect("bounded metadata row");
        assert!(bytes.len() <= 512);
        assert_eq!(
            client_preload_metadata::parse_ready(&bytes, &nonce, identity),
            Ok(())
        );

        let after = fs::symlink_metadata(&metadata_path).expect("metadata path identity");
        assert_eq!(after.dev(), metadata.device());
        assert_eq!(after.ino(), metadata.inode());
        assert_eq!(after.nlink(), 1);
        assert_eq!(
            fs::read(&source).expect("fixed source remains readable"),
            REPORTER_SOURCE
        );
        assert_eq!(
            fs::symlink_metadata(&source).unwrap().ino(),
            source_stamp.ino()
        );
        root.validate_topology()
            .expect("owned offline fixture topology");
        drop(metadata);
        fs::remove_dir_all(root.root()).expect("remove successful owned fixture root");
    }

    fn write_source(root: &FixtureRoot) -> (std::path::PathBuf, fs::Metadata) {
        let source = root.protected().join("preload_probe.cjs");
        let mut source_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&source)
            .expect("exclusive fixed reporter source");
        source_file
            .write_all(REPORTER_SOURCE)
            .expect("write fixed source");
        drop(source_file);
        let source_stamp = fs::symlink_metadata(&source).expect("source metadata");
        assert!(source_stamp.is_file() && !source_stamp.file_type().is_symlink());
        assert_eq!(source_stamp.permissions().mode() & 0o777, 0o600);
        (source, source_stamp)
    }

    fn launch(
        root: &FixtureRoot,
        node: &Path,
        source: &Path,
        metadata_path: &Path,
        metadata: &ObserverFile,
        nonce: &str,
    ) -> actors::Captured {
        let env = [
            ("HOME", root.runtime().to_string_lossy().into_owned()),
            (
                "HARFLOW_TLS_OBSERVER_FILE",
                metadata_path.to_string_lossy().into_owned(),
            ),
            ("HARFLOW_TLS_OBSERVER_NONCE", nonce.to_owned()),
            ("HARFLOW_TLS_OBSERVER_DEV", metadata.device().to_string()),
            ("HARFLOW_TLS_OBSERVER_INODE", metadata.inode().to_string()),
        ]
        .into_iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect();
        let until = Instant::now() + Duration::from_secs(5);
        let spec = SpawnSpec {
            executable: node.to_owned(),
            cwd: root.workspace(),
            args: vec![source.as_os_str().to_owned()],
            environment: env,
            capture_limit: 512,
        };
        let _cwd = FixtureCwd::enter(&spec.cwd);
        let child = RetainedOwnedChild::spawn(spec, &MacIdentity, until)
            .expect("retained local Node child");
        let capture = actors::Original::new(child).collect(until);
        drop(_cwd);
        capture
    }

    struct FixtureCwd(std::path::PathBuf);
    impl FixtureCwd {
        fn enter(path: &std::path::Path) -> Self {
            let previous = std::env::current_dir().expect("controller current directory");
            std::env::set_current_dir(path).expect("fixture workspace current directory");
            Self(previous)
        }
    }
    impl Drop for FixtureCwd {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.0).expect("restore controller current directory");
        }
    }
}
