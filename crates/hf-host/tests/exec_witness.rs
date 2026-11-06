#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_application::process::IdentityPort;
use hf_domain::{
    fixture::{EffectStart, Frame},
    ownership::{Fence, Id, OwnerClaim},
};
use hf_platform::process::{
    MacIdentity, RetainedOwnedChild, SpawnInitialization, SpawnSpec, WaitOutcome,
};
use hf_storage::{Limits, RootHold, Store};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

static WITNESS_TEST_LOCK: Mutex<()> = Mutex::new(());

fn deadline(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds)
}
fn id(ch: char) -> Id {
    Id::new(ch.to_string().repeat(32)).unwrap()
}
fn binary() -> PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("harflow-engine-scaffold")
}
fn fresh_root() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path =
        PathBuf::from("/private/tmp").join(format!("hf-exec-witness-{}-{n}", std::process::id()));
    fs::create_dir(&path).unwrap();
    fs::canonicalize(path).unwrap()
}

fn fixture_start(root: &Path) -> Frame {
    let store_path = root.join("store");
    let store = Store::initialize_fixture(
        &store_path,
        Limits::default(),
        deadline(5),
        hf_platform::lease::shared(),
    )
    .unwrap();
    drop(store);
    let held = RootHold::open(&store_path, deadline(5), hf_platform::lease::shared()).unwrap();
    let generation = held.source_stamp().generation.clone();
    drop(held);
    let caller = MacIdentity
        .identity(std::process::id())
        .expect("current parent identity");
    let operation = id('1');
    Frame::Start(EffectStart {
        operation: operation.clone(),
        root: store_path.to_string_lossy().into_owned(),
        ledger: hf_domain::fixture::QualifiedLedger {
            device: 1,
            inode: 1,
        },
        fence: Fence {
            operation,
            generation,
            attempt: id('2'),
            owner: OwnerClaim {
                owner: id('3'),
                nonce: id('4'),
                process: caller,
            },
            revision: 1,
        },
    })
}

fn start_bridge(root: &Path, start: &[u8], close_stdin: bool) -> RetainedOwnedChild {
    let _cwd = CurrentDirectory::enter(&root.to_path_buf());
    let mut child = RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: binary(),
            cwd: root.to_path_buf(),
            args: vec!["--fixture-exec-bridge-witness-v1".into()],
            environment: Vec::<(OsString, OsString)>::new(),
            capture_limit: 4096,
        },
        &MacIdentity,
        deadline(5),
    )
    .unwrap();
    assert_eq!(child.initialization(), SpawnInitialization::Ready);
    child.write_fixture_frame(start, deadline(3)).unwrap();
    if close_stdin {
        child.close_stdin();
    }
    child
}

fn require_refused_before_exec(mut child: RetainedOwnedChild, until: Instant, entry_ack: bool) {
    if entry_ack {
        let expected = child.registered_identity().unwrap().clone();
        assert_eq!(
            hf_interfaces::fixture::decode(&child.read_fixture_frame(deadline(3)).unwrap())
                .unwrap(),
            Frame::Hello(expected),
            "complete Start receives only its pre-exec identity acknowledgement"
        );
    }
    assert_eq!(
        child.wait_until(until, || false).unwrap(),
        WaitOutcome::Exited(hf_domain::process::ProcessExit::Code(2))
    );
    while !child.capture().stdout.eof || !child.capture().stderr.eof {
        child.pump(deadline(3), 4096).unwrap();
    }
    let (stdout, stderr) = child.take_capture_bytes();
    assert!(
        stdout.is_empty(),
        "refused bridge emitted no post-exec witness"
    );
    assert!(
        stderr.is_empty(),
        "refusal does not disclose inherited environment"
    );
}

struct CurrentDirectory(PathBuf);
impl CurrentDirectory {
    fn enter(path: &PathBuf) -> Self {
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self(previous)
    }
}
impl Drop for CurrentDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

#[test]
fn source_owned_exec_image_preserves_original_identity_and_settles_stdio_only() {
    let _guard = WITNESS_TEST_LOCK.lock().unwrap();
    let root = fresh_root();
    let start = fixture_start(&root);
    let _cwd = CurrentDirectory::enter(&root);
    let mut child = RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: binary(),
            cwd: root.clone(),
            args: vec!["--fixture-exec-bridge-witness-v1".into()],
            environment: Vec::<(OsString, OsString)>::new(),
            capture_limit: 4096,
        },
        &MacIdentity,
        deadline(5),
    )
    .unwrap();
    assert_eq!(child.initialization(), SpawnInitialization::Ready);
    let identity = child.registered_identity().unwrap().clone();
    child
        .write_fixture_frame(
            &hf_interfaces::fixture::encode(&start).unwrap(),
            deadline(3),
        )
        .unwrap();
    child.close_stdin();
    let hello =
        hf_interfaces::fixture::decode(&child.read_fixture_frame(deadline(5)).unwrap()).unwrap();
    assert_eq!(
        hello,
        Frame::Hello(identity.clone()),
        "post-exec image reports the original process identity"
    );
    let image_hello =
        hf_interfaces::fixture::decode(&child.read_fixture_frame(deadline(5)).unwrap()).unwrap();
    assert_eq!(
        image_hello,
        Frame::Hello(identity),
        "fixed exec image keeps identity"
    );
    assert_eq!(
        child.wait_until(deadline(5), || false).unwrap(),
        WaitOutcome::Exited(hf_domain::process::ProcessExit::Code(0))
    );
    while !child.capture().stdout.eof || !child.capture().stderr.eof {
        child.pump(deadline(5), 4096).unwrap();
    }
    let (stdout, stderr) = child.take_capture_bytes();
    assert_eq!(
        stdout,
        root.to_string_lossy().as_bytes(),
        "post-exec CWD is exact fixture root"
    );
    assert!(
        stderr.is_empty(),
        "witness does not print environment or diagnostics"
    );
    assert!(
        child.capture().stdout.eof && child.capture().stderr.eof,
        "both original pipes reached EOF"
    );
    drop(child);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn partial_start_cannot_reach_the_fixed_exec_image() {
    let _guard = WITNESS_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let root = fresh_root();
    let bytes = hf_interfaces::fixture::encode(&fixture_start(&root)).unwrap();
    let child = start_bridge(&root, &bytes[..bytes.len() - 1], true);
    require_refused_before_exec(child, deadline(6), false);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn complete_start_without_eof_cannot_reach_the_fixed_exec_image() {
    let _guard = WITNESS_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let root = fresh_root();
    let bytes = hf_interfaces::fixture::encode(&fixture_start(&root)).unwrap();
    let child = start_bridge(&root, &bytes, false);
    require_refused_before_exec(child, deadline(6), true);
    fs::remove_dir_all(root).unwrap();
}
