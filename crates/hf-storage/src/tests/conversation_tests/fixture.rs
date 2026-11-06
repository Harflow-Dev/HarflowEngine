#![allow(dead_code)]
use crate::{Limits, Store};
use hf_application::conversation::Command;
use hf_domain::{
    conversation::{
        BoundaryEvidence, CancelIntent, Claim, ClaimIntent, Enrollment, Ledger, Receipt, SendIntent,
    },
    ownership::{
        CaptureAudit, ClaimResult, Digest, Fence, Id, OwnerClaim, PrepareResult, PreparedRequest,
        ReleaseProof,
    },
    process::{ProcessExit, ProcessIdentity},
};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest as _, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A private temporary directory. Each value gets a distinct path, so one test
/// may hold several fixtures.
pub(super) struct Fixture(PathBuf);
impl Fixture {
    pub fn new(label: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("hf-r06-{}-{label}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
    pub fn root(&self) -> PathBuf {
        self.0.join("store")
    }
    pub fn store(&self) -> Store {
        self.store_with(limits())
    }
    pub fn store_with(&self, limits: Limits) -> Store {
        Store::initialize_fixture(
            self.root(),
            limits,
            deadline(),
            crate::tests::lease::lease_port(),
        )
        .unwrap()
    }
    pub fn reopen(&self) -> Store {
        self.reopen_with(limits())
    }
    pub fn reopen_with(&self, limits: Limits) -> Store {
        Store::open(
            self.root(),
            limits,
            deadline(),
            crate::tests::lease::lease_port(),
        )
        .unwrap()
    }
    pub fn raw(&self) -> Connection {
        Connection::open_with_flags(
            self.root().join("store.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    }
    /// Write connection; use only while no `Store` is open on this fixture.
    pub fn raw_write(&self) -> Connection {
        Connection::open(self.root().join("store.sqlite3")).unwrap()
    }
    pub fn value(&self, key: &str) -> Option<Vec<u8>> {
        use rusqlite::OptionalExtension;
        self.raw()
            .query_row(
                "SELECT value FROM fixture_records WHERE key=?1",
                [key],
                |r| r.get(0),
            )
            .optional()
            .unwrap()
    }
    pub fn ledger_bytes(&self, e: &Enrollment) -> Vec<u8> {
        self.value(&ledger_key(e)).unwrap()
    }
    pub fn private_rows(&self) -> Vec<(String, Vec<u8>)> {
        let raw = self.raw();
        let mut statement = raw
            .prepare("SELECT key,value FROM fixture_records WHERE key LIKE '__r06/%' ORDER BY key")
            .unwrap();
        statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn limits() -> Limits {
    Limits {
        max_rows: 128,
        max_bytes: 4 * 1024 * 1024,
    }
}
pub(super) fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(4)
}
pub(super) fn id(n: u32) -> Id {
    Id::new(format!("{n:032x}")).unwrap()
}
pub(super) fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 12, 34, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}
pub(super) fn ledger_key(e: &Enrollment) -> String {
    format!("__r06/ledger/{}", e.conversation.as_str())
}
pub(super) fn source_key(e: &Enrollment) -> String {
    format!("__r06/source/{}", e.source.operation.as_str())
}
pub(super) fn context_key(e: &Enrollment) -> String {
    format!("__r06/context/{}", e.context.as_str())
}

/// Creates an accepted source operation and returns its enrollment.
pub(super) fn seed(store: &Store) -> Enrollment {
    let payload = b"r06 source request".to_vec();
    let source = PreparedRequest {
        operation: id(1),
        session: id(2),
        reservation: id(3),
        attempt: id(4),
        generation: Id::new(store.identity().generation.clone()).unwrap(),
        digest: Digest::new(format!("{:x}", Sha256::digest(&payload))).unwrap(),
        payload,
    };
    assert!(matches!(
        store.ownership_prepare(&source, deadline()).unwrap(),
        PrepareResult::Prepared(_)
    ));
    let owner = OwnerClaim {
        owner: id(5),
        nonce: id(6),
        process: process(101),
    };
    let ClaimResult::Granted(claimed) = store
        .ownership_claim(&source.operation, &source.generation, 0, &owner, deadline())
        .unwrap()
    else {
        panic!("expected source claim")
    };
    let child = process(102);
    let spawned = store
        .ownership_record_spawn(&claimed.fence().unwrap(), &child, deadline())
        .unwrap();
    let accepted = store
        .ownership_record_acceptance(&spawned.fence().unwrap(), deadline())
        .unwrap();
    Enrollment {
        conversation: id(7),
        head_task: id(8),
        fence: accepted.fence().unwrap(),
        source,
        child,
        context: id(9),
        profile: Digest::new("a".repeat(64)).unwrap(),
        generation: 12,
        turn: id(11),
        ordinary_turns: 14,
        node_attempts: 6,
    }
}

/// Seeds, enrolls and optionally queues messages (ids 30..) and a boundary
/// (evidence 12). The store is dropped before returning.
pub(super) fn prime(fixture: &Fixture, queued: u32, boundary: bool) -> Enrollment {
    let store = fixture.store();
    let enrollment = seed(&store);
    store.conversation_enroll(&enrollment, deadline()).unwrap();
    for n in 0..queued {
        let ledger = observe(&store, &enrollment);
        apply(
            &store,
            &enrollment,
            Command::Send(send_intent(&ledger, 30 + n, "m")),
        );
    }
    if boundary {
        let ledger = observe(&store, &enrollment);
        apply(
            &store,
            &enrollment,
            Command::Boundary(evidence(&ledger, 12)),
        );
    }
    enrollment
}
pub(super) fn observe(store: &Store, e: &Enrollment) -> Ledger {
    store
        .conversation_observe(&e.conversation, deadline())
        .unwrap()
}
pub(super) fn apply(store: &Store, e: &Enrollment, command: Command) -> Ledger {
    store
        .conversation_apply(&e.conversation, command, deadline())
        .unwrap()
        .ledger
}
pub(super) fn evidence(ledger: &Ledger, n: u32) -> BoundaryEvidence {
    BoundaryEvidence {
        expected: ledger.witness(),
        context: ledger.enrollment.context.clone(),
        profile: ledger.enrollment.profile.clone(),
        evidence: id(n),
    }
}
pub(super) fn send_intent(ledger: &Ledger, request: u32, text: &str) -> SendIntent {
    SendIntent {
        request: id(request),
        expected: ledger.witness(),
        text: text.into(),
    }
}
pub(super) fn claim_intent(ledger: &Ledger, request: u32, destination: u32) -> ClaimIntent {
    ClaimIntent {
        request: id(request),
        expected: ledger.witness(),
        destination: id(destination),
    }
}
pub(super) fn cancel_intent(ledger: &Ledger, request: u32, message: u32) -> CancelIntent {
    CancelIntent {
        request: id(request),
        message: id(message),
        expected: ledger.witness(),
    }
}
pub(super) fn receipt(claim: Claim, n: u32) -> Command {
    Command::Receipt(Receipt {
        claim,
        evidence: id(n),
    })
}

/// Releases the source operation with terminal proof, as an owner would after exit.
pub(super) fn release_source(store: &Store, e: &Enrollment) {
    let proof = ReleaseProof::Terminal {
        evidence: id(60),
        owner_dead: true,
        child: e.child.clone(),
        exit: ProcessExit::Code(0),
        drain_complete: true,
        containment_complete: true,
        capture: CaptureAudit::Lossless,
    };
    store
        .ownership_release(
            &e.source.operation,
            &e.source.generation,
            e.fence.revision,
            Some(&e.fence as &Fence),
            &proof,
            deadline(),
        )
        .unwrap();
}

/// Sends then cancels `pairs` messages through the store. Request ids start at `base`.
pub(super) fn churn(store: &Store, e: &Enrollment, base: u32, pairs: u32) -> Ledger {
    let mut ledger = observe(store, e);
    for n in 0..pairs {
        let message = base + 2 * n;
        ledger = apply(store, e, Command::Send(send_intent(&ledger, message, "c")));
        let cancel = cancel_intent(&ledger, message + 1, message);
        ledger = apply(store, e, Command::Cancel(cancel));
    }
    ledger
}
