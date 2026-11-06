//! Experiments on WAL file size policy and sidecar retention.
use super::{common::lease::lease_port, data, ops, util::*};
use hf_storage::{Mutation, Store};
use std::path::Path;

/// WAL file high-water behaviour after PASSIVE checkpoint, with and without a
/// journal_size_limit, followed by writes that restart the log.
pub fn wal_size_policy() {
    let mut file = out("wal-size.jsonl");
    for limit in [0i64, 4 * 1024 * 1024] {
        let work = fresh("walsize");
        let root = work.join("store");
        cp(&dir().join("ci-wal"), &root);
        let conn = rusqlite::Connection::open(root.join("store.sqlite3")).unwrap();
        conn.set_db_config(
            rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
            true,
        )
        .unwrap();
        conn.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        if limit > 0 {
            conn.pragma_update(None, "journal_size_limit", limit)
                .unwrap();
        }
        let size = || {
            std::fs::metadata(root.join("store.sqlite3-wal"))
                .unwrap()
                .len()
        };
        let start = size();
        let (busy, log, done): (i64, i64, i64) = conn
            .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        let after_ckpt = size();
        conn.execute(
            "INSERT OR REPLACE INTO fixture_records VALUES('x',x'00')",
            [],
        )
        .unwrap();
        let after_write = size();
        record(
            &mut file,
            &[
                ("journal_size_limit", limit.to_string()),
                ("wal_start", start.to_string()),
                ("passive_busy_log_done", format!("{busy}/{log}/{done}")),
                ("wal_after_passive_checkpoint", after_ckpt.to_string()),
                ("wal_after_next_write", after_write.to_string()),
            ],
        );
    }
}

fn sizes(dir: &Path) -> String {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            format!(
                "{}:{}",
                e.file_name().to_string_lossy(),
                e.metadata().unwrap().len()
            )
        })
        .collect();
    v.sort();
    v.join(",")
}

/// Which sidecars exist after each kind of clean close, and after recover.
pub fn shm_retention() {
    let work = fresh("retention");
    let root = work.join("store");
    let mut file = out("retention.jsonl");
    let mut note = |step: &str| {
        record(
            &mut file,
            &[("step", step.into()), ("files_and_sizes", sizes(&root))],
        )
    };
    let store =
        Store::initialize_fixture(&root, data::limits(), ops::deadline(), lease_port()).unwrap();
    note("initialized_and_open");
    let put = [Mutation::Put {
        key: "a".into(),
        value: vec![1; 100],
    }];
    store.apply_batch(&put, ops::deadline()).unwrap();
    drop(store);
    note("after_clean_close_of_engine_handle_with_commits");
    drop(ops::ro_open(&root).unwrap());
    note("after_clean_close_of_read_only_reader");
    drop(Store::open(&root, data::limits(), ops::deadline(), lease_port()).unwrap());
    note("after_reopen_and_close_without_writes");
    Store::recover(&root, ops::deadline(), &work, lease_port()).unwrap();
    note("after_recover_truncate");
    drop(ops::ro_open(&root).unwrap());
    note("after_reader_close_following_recover");
}
