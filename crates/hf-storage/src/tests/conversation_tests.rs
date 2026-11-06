#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::{Limits, Store};
use crate::Mutation;
use hf_application::conversation::Command;
use hf_domain::{
    conversation::{Enrollment, MessageState, SendIntent},
    ownership::{ClaimResult, Digest, Id, OwnerClaim, PrepareResult, PreparedRequest},
    process::ProcessIdentity,
};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest as _, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command as ProcessCommand, Stdio},
    time::{Duration, Instant},
};

struct Fixture(
    PathBuf,
    #[allow(dead_code)] std::sync::MutexGuard<'static, ()>,
);
impl Fixture {
    fn new() -> Self {
        let guard = crate::tests::fixture_guard();
        let path = std::env::temp_dir().join(format!("hf-r06-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap(), guard)
    }
    fn root(&self) -> PathBuf {
        self.0.join("store")
    }
    fn store(&self) -> Store {
        Store::initialize_fixture(
            self.root(),
            Limits {
                max_rows: 128,
                max_bytes: 4 * 1024 * 1024,
            },
            deadline(),
            crate::tests::lease::lease_port(),
        )
        .unwrap()
    }
    fn raw(&self) -> Connection {
        Connection::open_with_flags(
            self.root().join("store.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Actor(Child);
impl Actor {
    fn spawn(fixture: &Fixture, hook: &str) -> Self {
        let child = ProcessCommand::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("tests::conversation_tests::conversation_actor_child")
            .arg("--nocapture")
            .env("HF_R06_ROOT", fixture.root())
            .env("HF_R06_HOOK", hook)
            .env("HF_R06_ACTOR", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        Self(child)
    }
    fn wait_stage(&mut self, hook: &str) {
        let mut lines = BufReader::new(self.0.stdout.as_mut().unwrap());
        let mut line = String::new();
        loop {
            line.clear();
            assert!(
                lines.read_line(&mut line).unwrap() > 0,
                "actor failed to become ready"
            );
            if line.trim() == "ACTOR_READY" {
                break;
            }
        }
        self.0.stdin.as_mut().unwrap().write_all(b"GO\n").unwrap();
        self.0.stdin.as_mut().unwrap().flush().unwrap();
        loop {
            line.clear();
            assert!(
                lines.read_line(&mut line).unwrap() > 0,
                "actor exited before stage"
            );
            if line.trim() == format!("STAGE:{hook}") {
                break;
            }
        }
    }
    fn kill(mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(4)
}
fn id(n: u8) -> Id {
    Id::new(format!("{n:032x}")).unwrap()
}
fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 12, 34, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}
fn seed(store: &Store) -> Enrollment {
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

#[test]
fn accepted_source_enrollment_and_queue_state_survive_close_reopen_with_independent_sqlite_oracle()
{
    let fixture = Fixture::new();
    let store = fixture.store();
    let enrollment = seed(&store);
    let initial = store.conversation_enroll(&enrollment, deadline()).unwrap();
    assert_eq!(initial.enrollment.source, enrollment.source);
    let witness = initial.witness();
    let intent = SendIntent {
        request: id(10),
        expected: witness,
        text: " \n\0 exact UTF-8 λ".into(),
    };
    store
        .conversation_apply(&enrollment.conversation, Command::Send(intent), deadline())
        .unwrap();
    let raw = fixture.raw();
    let keys: i64 = raw
        .query_row(
            "SELECT count(*) FROM fixture_records WHERE key LIKE '__r06/%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        keys, 3,
        "ledger and two immutable indexes are independently visible"
    );
    let ledger = raw
        .query_row(
            "SELECT value FROM fixture_records WHERE key=?1",
            [format!("__r06/ledger/{}", enrollment.conversation.as_str())],
            |r| r.get::<_, Vec<u8>>(0),
        )
        .unwrap();
    assert!(ledger.starts_with(b"HFCQ1"));
    drop(raw);
    drop(store);
    let reopened = Store::open(
        fixture.root(),
        Limits {
            max_rows: 128,
            max_bytes: 4 * 1024 * 1024,
        },
        deadline(),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    let observed = reopened
        .conversation_observe(&enrollment.conversation, deadline())
        .unwrap();
    assert_eq!(observed.messages.len(), 1);
    assert_eq!(observed.messages[0].state, MessageState::Queued);
    assert_eq!(observed.messages[0].intent.text, " \n\0 exact UTF-8 λ");
    assert_eq!(
        (
            observed.enrollment.ordinary_turns,
            observed.enrollment.node_attempts
        ),
        (14, 6)
    );
}

#[test]
fn generic_batch_cannot_modify_private_conversation_namespace() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let enrollment = seed(&store);
    store.conversation_enroll(&enrollment, deadline()).unwrap();
    for mutation in [
        Mutation::Insert {
            key: format!("__r06/ledger/{}", enrollment.conversation.as_str()),
            value: vec![0],
        },
        Mutation::Put {
            key: format!("__r06/source/{}", enrollment.source.operation.as_str()),
            value: vec![1],
        },
        Mutation::Delete {
            key: format!("__r06/context/{}", enrollment.context.as_str()),
        },
    ] {
        assert!(matches!(
            store.apply_batch(&[mutation], deadline()),
            Err(super::super::Error::InvalidOwnership)
        ));
    }
    assert_eq!(
        store
            .conversation_observe(&enrollment.conversation, deadline())
            .unwrap()
            .messages
            .len(),
        0
    );
}

#[test]
fn conversation_transaction_cut_is_atomic_before_and_after_commit() {
    for (hook, committed) in [
        ("conversation_before_commit", false),
        ("conversation_after_commit", true),
    ] {
        let fixture = Fixture::new();
        let store = fixture.store();
        let enrollment = seed(&store);
        store.conversation_enroll(&enrollment, deadline()).unwrap();
        drop(store);
        let baseline = raw_ledger(&fixture, &enrollment);
        let mut actor = Actor::spawn(&fixture, hook);
        actor.wait_stage(hook);
        assert_eq!(
            raw_ledger(&fixture, &enrollment) != baseline,
            committed,
            "visibility at {hook}"
        );
        actor.kill();
        let reopened = Store::open(
            fixture.root(),
            Limits {
                max_rows: 128,
                max_bytes: 4 * 1024 * 1024,
            },
            deadline(),
            crate::tests::lease::lease_port(),
        )
        .unwrap();
        assert_eq!(
            reopened
                .conversation_observe(&enrollment.conversation, deadline())
                .unwrap()
                .messages
                .len(),
            usize::from(committed)
        );
    }
}
fn raw_ledger(fixture: &Fixture, enrollment: &Enrollment) -> Vec<u8> {
    fixture
        .raw()
        .query_row(
            "SELECT value FROM fixture_records WHERE key=?1",
            [format!("__r06/ledger/{}", enrollment.conversation.as_str())],
            |r| r.get(0),
        )
        .unwrap()
}
#[test]
fn conversation_actor_child() {
    let Ok(root) = std::env::var("HF_R06_ROOT") else {
        return;
    };
    let hook = std::env::var("HF_R06_HOOK").unwrap();
    let store = Store::open(
        root,
        Limits {
            max_rows: 128,
            max_bytes: 4 * 1024 * 1024,
        },
        deadline(),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    crate::tests::select_stage(hook);
    println!("ACTOR_READY");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    let conversation = id(7);
    let ledger = store
        .conversation_observe(&conversation, deadline())
        .unwrap();
    store
        .conversation_apply(
            &conversation,
            Command::Send(SendIntent {
                request: id(30),
                expected: ledger.witness(),
                text: "actor".into(),
            }),
            deadline(),
        )
        .unwrap();
}
