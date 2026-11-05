mod actor;
pub(super) use actor::{Actor, actor_child};

use super::super::Store;
use hf_domain::{
    capture::{CaptureBinding, FileIdentity},
    ownership::{ClaimResult, Digest, Id, OwnerClaim, PrepareResult, PreparedRequest},
    process::ProcessIdentity,
};
use sha2::{Digest as _, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::MutexGuard,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(8);
pub(super) fn deadline() -> Instant {
    Instant::now() + WAIT
}

pub(super) struct Fixture(PathBuf, #[allow(dead_code)] MutexGuard<'static, ()>);
impl Fixture {
    pub(super) fn new() -> Self {
        let guard = crate::tests::fixture_guard();
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-capture-atomic-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap(), guard)
    }
    pub(super) fn root(&self) -> PathBuf {
        self.0.join("store")
    }
    pub(super) fn readonly(&self) -> rusqlite::Connection {
        rusqlite::Connection::open_with_flags(
            self.root().join("store.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn accepted_binding(store: &Store, tag: u8) -> CaptureBinding {
    let payload = b"atomicity actor fixture".to_vec();
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
        panic!("owner claim")
    };
    let child = process(200 + u32::from(tag));
    let spawned = store
        .ownership_record_spawn(&claimed.fence().unwrap(), &child, deadline())
        .unwrap();
    let accepted = store
        .ownership_record_acceptance(&spawned.fence().unwrap(), deadline())
        .unwrap();
    CaptureBinding {
        fence: accepted.fence().unwrap(),
        child,
        stdout: FileIdentity {
            device: 1,
            inode: 1001,
        },
        stderr: FileIdentity {
            device: 1,
            inode: 1002,
        },
    }
}
fn id(value: u8) -> Id {
    Id::new(format!("{value:032x}")).unwrap()
}
fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 12, 34, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}
