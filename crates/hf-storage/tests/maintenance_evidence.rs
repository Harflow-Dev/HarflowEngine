#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Owner evidence read under a maintenance session and judged with a scripted identity
//! port: which recorded processes make a recovery refuse, and what basis the rows carry.
mod common;
use common::{Fixture, deadline, default_limits};
use hf_application::{
    ctl::state_recover::{
        RecoveryFsPort,
        evidence::{Basis, EvidenceSet, Refusal, Source, Subject, assess},
    },
    process::IdentityPort,
};
use hf_domain::{
    capture::FileIdentity,
    process::{IdentityObservation, ProcessIdentity},
};
use hf_storage::{EvidenceBounds, MaintenanceSession, SessionRequest, Store};
use std::{
    collections::{HashMap, HashSet},
    fs,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

const OP: &str = "0123456789abcdef0123456789abcdef";
const BOOT: &str = "abcdef01-2345-6789-abcd-ef0123456789";
const GENERATION_B: &str = "fedcba9876543210fedcba9876543210";

struct TestFs;
impl RecoveryFsPort for TestFs {
    fn create_lock(&self, root: &Path, name: &str) -> std::io::Result<bool> {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(root.join(name))
        {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(error),
        }
    }
    fn free_space(&self, _: &Path) -> std::io::Result<u64> {
        Ok(u64::MAX)
    }
}

/// A scripted port: the observation of each identity by pid (dead when unlisted), and the
/// pids that currently hold some process.
#[derive(Default)]
struct Script {
    observed: HashMap<u32, IdentityObservation>,
    held: HashSet<u32>,
}
impl Script {
    fn with(pid: u32, observation: IdentityObservation) -> Self {
        let mut script = Self::default();
        script.observed.insert(pid, observation);
        script
    }
}
impl IdentityPort for Script {
    fn identity(&self, pid: u32) -> Option<ProcessIdentity> {
        self.held
            .contains(&pid)
            .then(|| ProcessIdentity::new(pid, 1, 0, BOOT.into()).unwrap())
    }
    fn observe(&self, expected: &ProcessIdentity) -> IdentityObservation {
        self.observed
            .get(&expected.pid())
            .copied()
            .unwrap_or(IdentityObservation::Dead)
    }
}

fn store(scratch: &Fixture) -> PathBuf {
    let root = scratch.path().join("store");
    Store::initialize_fixture(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    root
}

fn id(path: &Path) -> FileIdentity {
    let metadata = fs::symlink_metadata(path).unwrap();
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn session(root: &Path) -> MaintenanceSession {
    let request = SessionRequest {
        root_identity: id(root),
        lock_identity: None,
    };
    MaintenanceSession::acquire_within(
        root,
        &request,
        Arc::new(TestFs),
        common::lease::lease_port(),
        Duration::from_millis(200),
    )
    .unwrap()
}

fn read(root: &Path) -> EvidenceSet {
    session(root)
        .evidence(OP, EvidenceBounds::default())
        .unwrap()
}

fn generation(root: &Path) -> String {
    let marker = fs::read_to_string(root.join("store.identity")).unwrap();
    marker.lines().nth(2).unwrap().to_owned()
}

fn sql(root: &Path, statements: &str) {
    let db = common::raw_connection(root.join("store.sqlite3"));
    db.execute_batch(statements).unwrap();
}

fn identity_text(pid: u32) -> String {
    format!("{pid}:1700000000:5:{BOOT}")
}

fn process_row(task: &str, pid: u32) -> String {
    format!(
        "INSERT INTO task_processes(task,pid,start_seconds,start_micros,boot_uuid) \
         VALUES('{task}',{pid},1700000000,5,'{BOOT}');"
    )
}

fn terminal_task(task: &str) -> String {
    format!(
        "INSERT INTO task_facts(id,valid,backend,repo_path,session_id,status,exit_code,\
         started_at,started_at_us,output_drained) \
         VALUES('{task}',1,'b','/r','s','completed',0,'t',1,1);"
    )
}

fn ownership(operation: &str, phase: i64, owner: Option<u32>, child: Option<u32>) -> String {
    let text =
        |pid: Option<u32>| pid.map_or("NULL".to_owned(), |pid| format!("'{}'", identity_text(pid)));
    format!(
        "INSERT INTO ownership_records(operation,session,reservation,attempt,generation,digest,\
         payload,phase,revision,cancelled,owner_process,child) \
         VALUES('{operation}','s-{operation}','r-{operation}','a-{operation}','g','d',x'00',\
         {phase},1,0,{},{});",
        text(owner),
        text(child)
    )
}

fn launch(task: &str, state: &str) -> String {
    format!(
        "INSERT INTO launch_attempts(name,task_id,generation,state,backend,has_resources,\
         has_worktree) VALUES('{task}','{task}','g','{state}','b',0,0);"
    )
}

fn write_fence(root: &Path, state: &str) {
    let generation = generation(root);
    fs::write(
        root.join("store.fence"),
        format!("harflow-fence-1\n{generation}\n{state}\n"),
    )
    .unwrap();
}

#[test]
fn terminal_task_with_live_process_is_active_owner() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    sql(&root, &(terminal_task("a") + &process_row("a", 4242)));
    let set = read(&root);
    assert_eq!(set.basis, Basis::Clear);
    let alive = Script::with(4242, IdentityObservation::Matching);
    assert_eq!(assess(&set, &alive), Err(Refusal::ActiveOwner));
}

#[test]
fn owner_process_alive_without_child_is_active_owner() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    sql(&root, &ownership("o1", 2, Some(4242), None));
    let set = read(&root);
    assert!(
        set.items
            .iter()
            .any(|item| item.source == Source::OwnershipOwner)
    );
    let alive = Script::with(4242, IdentityObservation::Matching);
    assert_eq!(assess(&set, &alive), Err(Refusal::ActiveOwner));
}

#[test]
fn claimed_without_child_is_ambiguous() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    sql(&root, &ownership("o1", 1, Some(4242), None));
    let set = read(&root);
    assert!(set.items.iter().any(|item| {
        item.source == Source::OwnershipChild && item.subject == Subject::Undecidable
    }));
    // The owner is dead, yet nothing says whether a child was spawned.
    assert_eq!(
        assess(&set, &Script::default()),
        Err(Refusal::AmbiguousOwner)
    );
    // Once the child is recorded the claim no longer is ambiguous.
    let scratch = Fixture::new();
    let root = store(&scratch);
    sql(&root, &ownership("o1", 1, Some(4242), Some(4243)));
    let set = read(&root);
    assert!(assess(&set, &Script::default()).is_ok());
}

#[test]
fn released_record_with_live_child_is_active_owner() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    sql(&root, &ownership("o1", 4, Some(4242), Some(4243)));
    let set = read(&root);
    assert!(
        set.items
            .iter()
            .any(|item| item.source == Source::OwnershipChild)
    );
    let alive = Script::with(4243, IdentityObservation::Matching);
    assert_eq!(assess(&set, &alive), Err(Refusal::ActiveOwner));
}

#[test]
fn dead_identities_pass() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    let statements = [
        terminal_task("a"),
        process_row("a", 4242),
        ownership("o1", 4, Some(4300), Some(4301)),
    ]
    .concat();
    sql(&root, &statements);
    let set = read(&root);
    assert_eq!(set.items.len(), 3);
    let port = Script::with(4301, IdentityObservation::Mismatch);
    let evidence = assess(&set, &port).unwrap();
    assert_eq!(evidence.basis, Basis::Clear);
    let mut pids: Vec<u32> = evidence.checked.iter().map(ProcessIdentity::pid).collect();
    pids.sort_unstable();
    assert_eq!(pids, vec![4242, 4300, 4301]);
}

#[test]
fn undecidable_is_ambiguous() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    sql(&root, &(terminal_task("a") + &process_row("a", 4242)));
    let set = read(&root);
    let unknown = Script::with(4242, IdentityObservation::Unknown);
    assert_eq!(assess(&set, &unknown), Err(Refusal::AmbiguousOwner));
    // A recorded pid with no start evidence: a holder cannot be tied to it.
    let scratch = Fixture::new();
    let root = store(&scratch);
    sql(
        &root,
        "INSERT INTO task_processes(task,pid) VALUES('b',4343);",
    );
    let set = read(&root);
    let mut held = Script::default();
    held.held.insert(4343);
    assert_eq!(assess(&set, &held), Err(Refusal::AmbiguousOwner));
    // Whether or not anything holds the pid, it can never be tied to a process.
    assert_eq!(
        assess(&set, &Script::default()),
        Err(Refusal::AmbiguousOwner)
    );
    // A damaged task record cannot establish its own state.
    let scratch = Fixture::new();
    let root = store(&scratch);
    sql(&root, "INSERT INTO task_facts(id,valid) VALUES('c',0);");
    let set = read(&root);
    assert_eq!(
        assess(&set, &Script::default()),
        Err(Refusal::AmbiguousOwner)
    );
}

#[test]
fn launch_attempt_spawned_without_identity_is_ambiguous() {
    for state in ["claimed", "granted", "spawned"] {
        let scratch = Fixture::new();
        let root = store(&scratch);
        sql(&root, &launch("t1", state));
        let set = read(&root);
        assert!(
            set.items
                .iter()
                .any(|item| item.source == Source::LaunchAttempt),
            "{state}"
        );
        assert_eq!(
            assess(&set, &Script::default()),
            Err(Refusal::AmbiguousOwner),
            "{state}"
        );
    }
    // No process exists before `granted`, and a settled launch recorded its exit.
    for state in [
        "reserved",
        "waiting_for_resources",
        "not_started",
        "settled",
    ] {
        let scratch = Fixture::new();
        let root = store(&scratch);
        sql(&root, &launch("t1", state));
        let set = read(&root);
        assert!(set.items.is_empty(), "{state}");
    }
    // A matching process row supplies the identity, so the attempt is not ambiguous.
    let scratch = Fixture::new();
    let root = store(&scratch);
    sql(
        &root,
        &(launch("t1", "spawned") + &terminal_task("t1") + &process_row("t1", 4242)),
    );
    let set = read(&root);
    assert!(assess(&set, &Script::default()).is_ok());
}

#[test]
fn basis_earlier_snapshot_for_commit_lost() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    sql(&root, &process_row("a", 4242));
    write_fence(&root, "commit_lost");
    let set = read(&root);
    assert_eq!(set.basis, Basis::EarlierSnapshot);
    // The rows are still read and judged.
    assert_eq!(set.items.len(), 1);
    let alive = Script::with(4242, IdentityObservation::Matching);
    assert_eq!(assess(&set, &alive), Err(Refusal::ActiveOwner));
    // A `failed` fence alone does not make the rows older.
    write_fence(&root, "failed");
    assert_eq!(read(&root).basis, Basis::Clear);
    // A watermark that is absent leaves the store behind its acknowledgements.
    fs::remove_file(root.join("store.fence")).unwrap();
    fs::remove_file(root.join("store.watermark")).unwrap();
    assert_eq!(read(&root).basis, Basis::EarlierSnapshot);
}

#[test]
fn basis_earlier_snapshot_for_missing_wal_reads_a_copy_and_never_creates_wal() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    sql(
        &root,
        &(process_row("a", 4242) + "PRAGMA wal_checkpoint(TRUNCATE);"),
    );
    fs::remove_file(root.join("store.sqlite3-wal")).unwrap();
    let names = |path: &Path| {
        let mut names: Vec<_> = fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        names
    };
    let before_names = names(&root);
    let before_main = fs::read(root.join("store.sqlite3")).unwrap();
    let set = read(&root);
    assert_eq!(set.basis, Basis::EarlierSnapshot);
    // The row came from the main file's copy.
    assert_eq!(set.items.len(), 1);
    assert!(!root.join("store.sqlite3-wal").exists());
    assert_eq!(names(&root), before_names);
    assert_eq!(fs::read(root.join("store.sqlite3")).unwrap(), before_main);
    // The scratch directory is gone.
    assert_eq!(
        names(scratch.path()),
        vec![std::ffi::OsString::from("store")]
    );
}

#[test]
fn basis_unreadable_for_identity_mismatch() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    sql(
        &root,
        &format!(
            "UPDATE store_identity SET generation='{GENERATION_B}' WHERE singleton=1; {}",
            process_row("a", 4242)
        ),
    );
    let set = read(&root);
    assert_eq!(set.basis, Basis::Unreadable);
    assert!(set.items.is_empty());
    // An unreadable basis is ambiguous whatever the (empty) items say.
    assert_eq!(
        assess(&set, &Script::default()),
        Err(Refusal::AmbiguousOwner)
    );
}
