#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Writer connection settings per checkpoint policy, and maintenance against a
//! connection held by another operation of the same handle.
use crate::{JOURNAL_SIZE_LIMIT, Limits, Store, WalMaintenance, WriterOptions};
use rusqlite::config::DbConfig;
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}

/// An initialized root, then a writer handle reopened with `options`.
fn reopened(name: &str, options: WriterOptions) -> (Scratch, Store) {
    let parent = std::env::temp_dir().join(format!("hf-storage-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let scratch = Scratch(fs::canonicalize(parent).unwrap());
    let root = scratch.0.join("store");
    let leases = || crate::tests::lease::lease_port();
    drop(Store::initialize_fixture(&root, Limits::default(), deadline(), leases()).unwrap());
    let store = Store::open_with(&root, Limits::default(), deadline(), leases(), options).unwrap();
    (scratch, store)
}

/// `journal_size_limit`, `wal_autocheckpoint` and the no-checkpoint-on-close flag.
fn settings(store: &Store) -> (i64, i64, bool) {
    let connection = store.connection.lock().unwrap();
    let limit = connection
        .pragma_query_value(None, "journal_size_limit", |row| row.get(0))
        .unwrap();
    let auto = connection
        .pragma_query_value(None, "wal_autocheckpoint", |row| row.get(0))
        .unwrap();
    let no_close = connection
        .db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE)
        .unwrap();
    (limit, auto, no_close)
}

#[test]
fn owner_limits_the_journal_and_keeps_both_checkpoint_suppressions() {
    let (_scratch, store) = reopened("owner-settings", WriterOptions::owner());
    let limit = i64::try_from(JOURNAL_SIZE_LIMIT).unwrap();
    assert_eq!(settings(&store), (limit, 0, true));
}

#[test]
fn suppressed_keeps_the_default_journal_limit_and_both_suppressions() {
    let (_scratch, store) = reopened("suppressed-settings", WriterOptions::suppressed());
    let (limit, auto, no_close) = settings(&store);
    assert_ne!(limit, i64::try_from(JOURNAL_SIZE_LIMIT).unwrap());
    assert_eq!((auto, no_close), (0, true));
}

#[test]
fn a_held_connection_skips_maintenance_and_the_release_restores_it() {
    let (_scratch, store) = reopened("held-connection", WriterOptions::owner());
    let guard = store.connection.lock().unwrap();
    assert_eq!(
        store.maintain_wal(deadline()).unwrap(),
        WalMaintenance::Skipped
    );
    drop(guard);
    assert_eq!(
        store.maintain_wal(deadline()).unwrap(),
        WalMaintenance::Idle
    );
}

#[test]
fn a_held_connection_does_not_skip_suppressed_maintenance() {
    let (_scratch, store) = reopened("held-suppressed", WriterOptions::suppressed());
    let _guard = store.connection.lock().unwrap();
    assert_eq!(
        store.maintain_wal(deadline()).unwrap(),
        WalMaintenance::Idle
    );
}

/// Child: grows a pinned log past `PASSIVE_AFTER`, then commits once more and stops at
/// the maintenance checkpoint, where the parent replaces `io.lock`. The maintenance then
/// fails with a non-skippable error after the commit has landed.
#[test]
fn maintenance_failure_after_commit_child() {
    let Ok(root) = std::env::var("HF_WAL_MAINTENANCE_ROOT") else {
        return;
    };
    let leases = crate::tests::lease::lease_port();
    let options = WriterOptions::owner();
    let store = Store::open_with(&root, Limits::default(), deadline(), leases, options).unwrap();
    let reader = crate::tests::raw_connection(PathBuf::from(&root).join("store.sqlite3"));
    reader.execute_batch("BEGIN DEFERRED").unwrap();
    let _: i64 = reader
        .query_row("SELECT count(*) FROM fixture_records", [], |row| row.get(0))
        .unwrap();
    for fill in 0..9u8 {
        let value = vec![fill; 1_000_000];
        let put = crate::Mutation::Put {
            key: "bulk".into(),
            value,
        };
        store.apply_batch(&[put], deadline()).unwrap();
    }
    reader.execute_batch("COMMIT").unwrap();
    drop(reader);
    crate::tests::select_stage("before_checkpoint".into());
    let put = crate::Mutation::Put {
        key: "probe".into(),
        value: b"landed".to_vec(),
    };
    let write = store.apply_batch(&[put], Instant::now() + Duration::from_secs(10));
    let maintain = store.maintain_wal(deadline());
    println!("RESULT write={write:?} maintain={maintain:?}");
}

fn replace_io_lock(root: &std::path::Path) {
    let fresh = root.join("io.lock.fresh");
    fs::write(&fresh, b"").unwrap();
    fs::rename(fresh, root.join("io.lock")).unwrap();
}

#[test]
fn a_non_skippable_maintenance_failure_never_fails_the_committed_write() {
    use std::io::{BufRead, BufReader, Write};
    let (scratch, store) = reopened("maintenance-failure", WriterOptions::suppressed());
    drop(store);
    let root = scratch.0.join("store");
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::wal_owner_tests::maintenance_failure_after_commit_child",
            "--nocapture",
        ])
        .env("HF_WAL_MAINTENANCE_ROOT", &root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = super::StageChild(child);
    let mut lines = BufReader::new(child.0.stdout.take().unwrap()).lines();
    let staged = lines.by_ref().map_while(Result::ok);
    assert!(
        staged
            .take(64)
            .any(|line| line.trim() == "STAGE:before_checkpoint"),
        "the maintenance checkpoint was not reached"
    );
    replace_io_lock(&root);
    child.0.stdin.take().unwrap().write_all(b"\n").unwrap();
    let result = lines
        .by_ref()
        .map_while(Result::ok)
        .find_map(|line| line.strip_prefix("RESULT ").map(str::to_owned))
        .expect("no result line");
    lines.for_each(drop);
    assert!(child.0.wait().unwrap().success());
    assert_eq!(result, "write=Ok(()) maintain=Err(Replaced)");
    let leases = crate::tests::lease::lease_port();
    let reopened = Store::open(&root, Limits::default(), deadline(), leases).unwrap();
    let records = reopened.read_batch(&["probe".into()], deadline()).unwrap();
    assert_eq!(records[0].value, b"landed");
}

/// A handle whose watermark write or flush failed refuses every later write, while its
/// reads, PASSIVE checkpoints and maintenance calls continue.
#[test]
fn a_poisoned_handle_refuses_writes_and_still_reads_and_checkpoints() {
    let (_scratch, store) = reopened("poisoned", WriterOptions::owner());
    let put = |key: &str| {
        let mutation = crate::Mutation::Put {
            key: key.into(),
            value: b"v".to_vec(),
        };
        store.apply_batch(&[mutation], deadline())
    };
    put("before").unwrap();
    store
        .poisoned
        .store(true, std::sync::atomic::Ordering::Release);
    for _ in 0..2 {
        assert!(matches!(
            put("after"),
            Err(crate::Error::WatermarkUnrecorded)
        ));
    }
    assert!(store.checkpoint_passive(deadline()).unwrap().complete);
    assert_eq!(
        store.maintain_wal(deadline()).unwrap(),
        WalMaintenance::Idle
    );
    let keys = ["before".to_string(), "after".to_string()];
    let records = store.read_batch(&keys, deadline()).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].key, "before");
}
