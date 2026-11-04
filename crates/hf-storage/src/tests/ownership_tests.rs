#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::{Limits, Store};
use crate::Error;
#[path = "ownership_tests/release_schema.rs"]
mod release_schema;
#[path = "ownership_tests/stale_fence.rs"]
mod stale_fence;

use hf_domain::{
    ownership::{
        ClaimResult, Digest, Id, OwnerClaim, Phase, PrepareResult, PreparedRequest, ReleaseProof,
    },
    process::{ProcessExit, ProcessIdentity},
};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Fixture(
    PathBuf,
    #[allow(dead_code)] std::sync::MutexGuard<'static, ()>,
);
impl Fixture {
    fn new(label: &str) -> Self {
        let guard = crate::tests::fixture_guard();
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-own-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap(), guard)
    }
    fn store(&self) -> Store {
        Store::initialize_fixture(self.0.join("store"), Limits::default(), deadline()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(4)
}
fn id(value: u8) -> Id {
    Id::new(format!("{value:032x}")).unwrap()
}
fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 12, 34, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}
// Fixed SHA-256 values were generated independently with Python hashlib.
fn expected_payload_digest(payload: &[u8]) -> &'static str {
    match payload {
        b"independent payload bytes" => {
            "2e1a257e9c0009041e7108d9d01ba1af15f33b44e5041197cd90580c707a9b30"
        }
        b"one" => "7692c3ad3540bb803c020b3aee66cd8887123234ea0c6e7143c0add73ff431ed",
        b"two" => "3fc4ccfe745870e2c0d99f71f30ff0656c8dedd41cc1d7d3d376b0dbe685e2f3",
        b"new op" => "59bcf3a61fadd5cd35f8a27cf97247004311371edbcc7d648e0c0e90336ef5a7",
        b"payload" => "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5",
        b"stored identity bytes" => {
            "679907c859b8d4d1d7c8c64c276fb51976ddceefe3cb680289e6390758f3d16d"
        }
        _ => panic!("fixture payload has no independent digest vector"),
    }
}
fn request(
    store: &Store,
    op: u8,
    session: u8,
    reservation: u8,
    attempt: u8,
    payload: &[u8],
) -> PreparedRequest {
    PreparedRequest {
        operation: id(op),
        session: id(session),
        reservation: id(reservation),
        attempt: id(attempt),
        generation: Id::new(store.identity().generation.clone()).unwrap(),
        digest: Digest::new(expected_payload_digest(payload).to_owned()).unwrap(),
        payload: payload.to_vec(),
    }
}
fn owner() -> OwnerClaim {
    OwnerClaim {
        owner: id(9),
        nonce: id(10),
        process: process(101),
    }
}
fn raw_record_signature(store: &Store, operation: &Id) -> Vec<String> {
    let connection = store.connection.lock().unwrap();
    connection
        .query_row(
            "SELECT quote(operation),quote(session),quote(reservation),quote(attempt),quote(generation),quote(digest),hex(payload),CAST(phase AS TEXT),CAST(revision AS TEXT),CAST(cancelled AS TEXT),quote(owner),quote(nonce),quote(owner_process),quote(child),quote(release_proof) FROM ownership_records WHERE operation=?1",
            [operation.as_str()],
            |row| (0..15).map(|index| row.get(index)).collect(),
        )
        .unwrap()
}

#[test]
fn immutable_prepare_is_idempotent_but_never_regrants_claim() {
    let fixture = Fixture::new("immutable");
    let store = fixture.store();
    let request = request(&store, 2, 3, 4, 5, b"independent payload bytes");
    assert!(matches!(
        store.ownership_prepare(&request, deadline()).unwrap(),
        PrepareResult::Prepared(_)
    ));
    assert!(matches!(
        store.ownership_prepare(&request, deadline()).unwrap(),
        PrepareResult::Existing(_)
    ));
    let claim = store
        .ownership_claim(
            &request.operation,
            &request.generation,
            0,
            &owner(),
            deadline(),
        )
        .unwrap();
    let ClaimResult::Granted(snapshot) = claim else {
        panic!("first claim must grant")
    };
    assert_eq!(snapshot.phase, Phase::Claimed);
    let repeated = store
        .ownership_claim(
            &request.operation,
            &request.generation,
            1,
            &owner(),
            deadline(),
        )
        .unwrap();
    assert!(matches!(repeated, ClaimResult::NoGrant(_)));
    let mut changed = request.clone();
    changed.payload = b"two".to_vec();
    changed.digest = Digest::new(expected_payload_digest(&changed.payload).to_owned()).unwrap();
    assert!(matches!(
        store.ownership_prepare(&changed, deadline()),
        Err(Error::OwnershipConflict)
    ));
    let mut same_digest = request.clone();
    same_digest.payload[0] ^= 1;
    assert!(matches!(
        store.ownership_prepare(&same_digest, deadline()),
        Err(Error::InvalidOwnership)
    ));
    let mut forged = request.clone();
    forged.digest = Digest::new("0".repeat(64)).unwrap();
    forged.operation = id(11);
    assert!(matches!(
        store.ownership_prepare(&forged, deadline()),
        Err(Error::InvalidOwnership)
    ));
}

#[test]
fn prepare_rejects_oversize_payload_and_store_byte_budget_without_reservation() {
    let fixture = Fixture::new("payload-limit");
    let store = fixture.store();
    let mut oversized = request(&store, 2, 3, 4, 5, b"payload");
    oversized.payload = vec![0; 16 * 1024 + 1];
    assert!(matches!(
        store.ownership_prepare(&oversized, deadline()),
        Err(Error::LimitExceeded)
    ));
    assert!(
        store
            .ownership_observe(&oversized.operation, deadline())
            .unwrap()
            .is_none()
    );
    drop(store);
    let tight = Store::initialize_fixture(
        fixture.0.join("tight"),
        Limits {
            max_rows: 1,
            max_bytes: 64,
        },
        deadline(),
    )
    .unwrap();
    let bounded = request(&tight, 6, 7, 8, 9, b"one");
    assert!(matches!(
        tight.ownership_prepare(&bounded, deadline()),
        Err(Error::LimitExceeded)
    ));
    assert!(
        tight
            .ownership_observe(&bounded.operation, deadline())
            .unwrap()
            .is_none()
    );
}

#[test]
fn ordinary_observation_preserves_exact_row_and_connection_change_count() {
    let fixture = Fixture::new("observation");
    let store = fixture.store();
    let request = request(&store, 2, 3, 4, 5, b"payload");
    store.ownership_prepare(&request, deadline()).unwrap();
    store
        .ownership_claim(
            &request.operation,
            &request.generation,
            0,
            &owner(),
            deadline(),
        )
        .unwrap();
    let signature = raw_record_signature(&store, &request.operation);
    let changes = store.connection.lock().unwrap().total_changes();
    let first = store
        .ownership_observe(&request.operation, deadline())
        .unwrap()
        .unwrap();
    let second = store
        .ownership_observe(&request.operation, deadline())
        .unwrap()
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(raw_record_signature(&store, &request.operation), signature);
    assert_eq!(store.connection.lock().unwrap().total_changes(), changes);
}

#[test]
fn live_session_reservation_and_global_attempt_history_are_exclusive() {
    let fixture = Fixture::new("exclusive");
    let store = fixture.store();
    let first = request(&store, 2, 3, 4, 5, b"one");
    store.ownership_prepare(&first, deadline()).unwrap();
    let second = request(&store, 6, 3, 7, 8, b"two");
    assert!(matches!(
        store.ownership_prepare(&second, deadline()),
        Err(Error::OwnershipConflict)
    ));
    let duplicate_attempt = request(&store, 6, 9, 7, 5, b"two");
    assert!(matches!(
        store.ownership_prepare(&duplicate_attempt, deadline()),
        Err(Error::OwnershipConflict)
    ));
}

#[test]
fn cancellation_prevents_claim_and_only_exact_prepared_release_frees_session() {
    let fixture = Fixture::new("cancel");
    let store = fixture.store();
    let first = request(&store, 2, 3, 4, 5, b"payload");
    store.ownership_prepare(&first, deadline()).unwrap();
    let cancelled = store
        .ownership_cancel(&first.operation, &first.generation, 0, deadline())
        .unwrap();
    assert!(cancelled.cancelled);
    assert!(matches!(
        store
            .ownership_claim(&first.operation, &first.generation, 1, &owner(), deadline())
            .unwrap(),
        ClaimResult::NoGrant(_)
    ));
    let proof = ReleaseProof::Unclaimed { evidence: id(21) };
    let released = store
        .ownership_release(
            &first.operation,
            &first.generation,
            cancelled.revision,
            None,
            &proof,
            deadline(),
        )
        .unwrap();
    assert_eq!(released.phase, Phase::Released);
    assert!(matches!(
        store.ownership_prepare(&first, deadline()),
        Ok(PrepareResult::Existing(_))
    ));
    let second = request(&store, 6, 3, 7, 8, b"new op");
    assert!(matches!(
        store.ownership_prepare(&second, deadline()).unwrap(),
        PrepareResult::Prepared(_)
    ));
}

#[test]
fn uncertain_claimed_state_requires_positive_never_started_proof_to_release() {
    let fixture = Fixture::new("unknown-release");
    let store = fixture.store();
    let request = request(&store, 2, 3, 4, 5, b"payload");
    store.ownership_prepare(&request, deadline()).unwrap();
    let ClaimResult::Granted(claimed) = store
        .ownership_claim(
            &request.operation,
            &request.generation,
            0,
            &owner(),
            deadline(),
        )
        .unwrap()
    else {
        panic!("grant")
    };
    let fence = claimed.fence().unwrap();
    let unknown = ReleaseProof::NeverStarted {
        evidence: id(22),
        owner_dead: false,
    };
    assert!(matches!(
        store.ownership_release(
            &request.operation,
            &request.generation,
            fence.revision,
            Some(&fence),
            &unknown,
            deadline()
        ),
        Err(Error::InvalidOwnership)
    ));
    assert_eq!(
        store
            .ownership_observe(&request.operation, deadline())
            .unwrap()
            .unwrap()
            .phase,
        Phase::Claimed
    );
    let positive = ReleaseProof::NeverStarted {
        evidence: id(23),
        owner_dead: true,
    };
    let released = store
        .ownership_release(
            &request.operation,
            &request.generation,
            fence.revision,
            Some(&fence),
            &positive,
            deadline(),
        )
        .unwrap();
    assert_eq!(released.phase, Phase::Released);
}
