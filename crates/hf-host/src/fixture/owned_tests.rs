#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::*;
use hf_application::owner::OwnerSession;
use hf_domain::ownership::{Digest, OwnerClaim, PreparedRequest};
use hf_platform::process::{CaptureOrigin, ReaderView};
use hf_storage::{Limits, Store};
use std::{
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
};

const HELPER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../hf-platform/tests/fixtures/reader_writer.py"
);
const OUTPUT_SIZE: u64 = 262_144;

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-owner-unwind-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
    fn root(&self) -> PathBuf {
        self.0.join("store")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn request(store: &Store) -> PreparedRequest {
    PreparedRequest {
        operation: Id::new("11".repeat(16)).unwrap(),
        session: Id::new("22".repeat(16)).unwrap(),
        reservation: Id::new("33".repeat(16)).unwrap(),
        attempt: Id::new("44".repeat(16)).unwrap(),
        generation: Id::new(store.identity().generation.clone()).unwrap(),
        digest: Digest::new(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
        )
        .unwrap(),
        payload: Vec::new(),
    }
}

fn owner_session(scratch: &Scratch, until: Instant) -> OwnerSession<Store> {
    let store = Store::initialize_fixture(
        scratch.root(),
        Limits::default(),
        until,
        hf_platform::lease::shared(),
    )
    .unwrap();
    let request = request(&store);
    store.ownership_prepare(&request, until).unwrap();
    let owner = OwnerClaim {
        owner: Id::new("66".repeat(16)).unwrap(),
        nonce: Id::new("77".repeat(16)).unwrap(),
        process: MacIdentity.identity(std::process::id()).unwrap(),
    };
    let mut session = OwnerSession::new(store, request, owner, 0);
    session.claim(until).unwrap();
    session
}

fn wait_for_output(view: &ReaderView, witness: &std::path::Path) {
    let until = Instant::now() + Duration::from_secs(6);
    loop {
        let progress = view.snapshot();
        if witness.exists()
            && progress.stdout.read == OUTPUT_SIZE
            && progress.stderr.read == OUTPUT_SIZE
        {
            assert!(!progress.stdout.eof && !progress.stderr.eof);
            return;
        }
        assert!(Instant::now() < until, "fixed helper output deadline");
        std::thread::yield_now();
    }
}

#[test]
fn owner_unwind_settles_reader_and_child_before_releasing_store_root_lease() {
    let _serial = super::super::test_support::case_guard();
    let scratch = Scratch::new();
    let until = Instant::now() + Duration::from_secs(8);
    let mut session = owner_session(&scratch, until);

    let witness = scratch.0.join("producer.finished");
    let child = RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: PathBuf::from("/usr/bin/python3"),
            args: vec![
                "-I".into(),
                "-S".into(),
                HELPER.into(),
                witness.as_os_str().to_owned(),
            ],
            cwd: fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
            environment: Vec::new(),
            capture_limit: 1,
        },
        &MacIdentity,
        until,
    )
    .unwrap();
    let mut owned = OwnerChild::new(child, until);
    let child_identity = owned.child.registered_identity().unwrap().clone();
    session.record_child(&child_identity, until).unwrap();
    let origin = CaptureOrigin::from_spawn_observed(session.snapshot().unwrap()).unwrap();
    owned.reader = Some(
        owned
            .child
            .start_private_fixture_capture_reader(origin, until)
            .unwrap(),
    );
    let view = owned.reader.as_ref().unwrap().view();
    wait_for_output(&view, &witness);
    assert!(matches!(
        Store::recover(
            scratch.root(),
            Instant::now() + Duration::from_secs(2),
            &scratch.0,
            hf_platform::lease::shared()
        ),
        Err(hf_storage::Error::Busy)
    ));

    let pid = child_identity.pid();
    let result = catch_unwind(AssertUnwindSafe(move || {
        let _owned = owned;
        panic!("injected owner unwind after reader start");
    }));
    assert!(result.is_err());
    let final_value = view.final_observation().expect("unwind settled reader");
    assert!(final_value.stopped);
    assert!(!final_value.progress.stdout.eof && !final_value.progress.stderr.eof);
    assert!(
        MacIdentity.identity(pid).is_none(),
        "owner guard did not reap fixed child"
    );

    assert!(matches!(
        Store::recover(
            scratch.root(),
            Instant::now() + Duration::from_secs(2),
            &scratch.0,
            hf_platform::lease::shared()
        ),
        Err(hf_storage::Error::Busy)
    ));
    drop(session);
    assert!(matches!(
        Store::recover(
            scratch.root(),
            Instant::now() + Duration::from_secs(2),
            &scratch.0,
            hf_platform::lease::shared()
        ),
        Ok(checkpoint) if checkpoint.complete
    ));
}
