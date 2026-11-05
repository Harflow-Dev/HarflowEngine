use super::super::{Limits, Store};
use hf_domain::{
    capture::{CaptureBinding, FileIdentity},
    ownership::{ClaimResult, Digest, Id, OwnerClaim, PrepareResult, PreparedRequest},
    process::ProcessIdentity,
};
use sha2::{Digest as _, Sha256};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

pub(super) struct Fixture(
    PathBuf,
    #[allow(dead_code)] std::sync::MutexGuard<'static, ()>,
);
impl Fixture {
    pub(super) fn new(label: &str) -> Self {
        let guard = crate::tests::fixture_guard();
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-capture-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap(), guard)
    }
    pub(super) fn root(&self) -> PathBuf {
        self.0.join("store")
    }
    pub(super) fn store(&self) -> Store {
        Store::initialize_fixture(self.root(), Limits::default(), deadline()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub(super) fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(3)
}
pub(super) fn id(value: u8) -> Id {
    Id::new(format!("{value:032x}")).unwrap()
}
pub(super) fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 12, 34, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}
pub(super) fn accepted_binding(store: &Store, tag: u8) -> CaptureBinding {
    let payload = b"capture fixture request".to_vec();
    let request = PreparedRequest {
        operation: id(tag),
        session: id(tag + 1),
        reservation: id(tag + 2),
        attempt: id(tag + 3),
        generation: Id::new(store.identity().generation.clone()).unwrap(),
        digest: Digest::new(format!("{:x}", Sha256::digest(&payload))).unwrap(),
        payload,
    };
    assert!(matches!(
        store.ownership_prepare(&request, deadline()).unwrap(),
        PrepareResult::Prepared(_)
    ));
    let owner = OwnerClaim {
        owner: id(tag + 4),
        nonce: id(tag + 5),
        process: process(100 + u32::from(tag)),
    };
    let ClaimResult::Granted(claimed) = store
        .ownership_claim(
            &request.operation,
            &request.generation,
            0,
            &owner,
            deadline(),
        )
        .unwrap()
    else {
        panic!("fixture owner claim")
    };
    let claimed_fence = claimed.fence().unwrap();
    let child = process(200 + u32::from(tag));
    let spawned = store
        .ownership_record_spawn(&claimed_fence, &child, deadline())
        .unwrap();
    let accepted = store
        .ownership_record_acceptance(&spawned.fence().unwrap(), deadline())
        .unwrap();
    CaptureBinding {
        fence: accepted.fence().unwrap(),
        child,
        stdout: FileIdentity {
            device: 1,
            inode: 10 + u64::from(tag),
        },
        stderr: FileIdentity {
            device: 1,
            inode: 20 + u64::from(tag),
        },
    }
}
