//! Component replay of the existing open path, step by step, plus the contention
//! writer. The replay mirrors hf-storage/src/wal.rs and connection.rs; it is a
//! measurement aid, not a second implementation.
use super::{common::lease::lease_port, ops, run};
use hf_storage::{Mutation, Store};
use rusqlite::{Connection, OpenFlags, config::DbConfig};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    time::Instant,
};

fn copy(from: &Path, to: &Path) -> u64 {
    let mut input = File::open(from).unwrap();
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
        .unwrap();
    let mut buffer = [0; 64 * 1024];
    let mut total = 0;
    loop {
        let count = input.read(&mut buffer).unwrap();
        if count == 0 {
            return total;
        }
        total += count as u64;
        output.write_all(&buffer[..count]).unwrap();
    }
}

fn no_checkpoints(conn: &Connection) {
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
        .unwrap();
    conn.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
}

fn identity(conn: &Connection) -> String {
    let _: u32 = conn
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .unwrap();
    let _: u32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    conn.query_row(
        "SELECT generation FROM store_identity WHERE singleton=1",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

/// One replay; returns the JSON fields (microseconds) for each step.
pub fn replay(root: &Path, parent: &Path) -> String {
    let total = Instant::now();
    let step = Instant::now();
    ops::marker(root).unwrap();
    ops::header(&root.join("store.sqlite3")).unwrap();
    let pre = ops::micros(step);
    let scratch = parent.join(".hf-snapshot-replay");
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir(&scratch).unwrap();
    let step = Instant::now();
    let main_bytes = copy(&root.join("store.sqlite3"), &scratch.join("store.sqlite3"));
    let copy_main = ops::micros(step);
    let step = Instant::now();
    let wal_bytes = match root.join("store.sqlite3-wal").try_exists().unwrap() {
        true => copy(
            &root.join("store.sqlite3-wal"),
            &scratch.join("store.sqlite3-wal"),
        ),
        false => 0,
    };
    let copy_wal = ops::micros(step);
    let step = Instant::now();
    let copy_conn = Connection::open_with_flags(
        scratch.join("store.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .unwrap();
    no_checkpoints(&copy_conn);
    let copy_open = ops::micros(step);
    let step = Instant::now();
    identity(&copy_conn);
    let copy_identity = ops::micros(step);
    let step = Instant::now();
    let check: String = copy_conn
        .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
        .unwrap();
    let quick_check = ops::micros(step);
    assert_eq!(check, "ok");
    let step = Instant::now();
    drop(copy_conn);
    fs::remove_dir_all(&scratch).unwrap();
    let cleanup = ops::micros(step);
    let step = Instant::now();
    let source = Connection::open_with_flags(
        root.join("store.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .unwrap();
    no_checkpoints(&source);
    identity(&source);
    let _: String = source
        .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
        .unwrap();
    source.pragma_update(None, "synchronous", "FULL").unwrap();
    source.pragma_update(None, "foreign_keys", true).unwrap();
    let source_open = ops::micros(step);
    drop(source);
    format!(
        r#""main_bytes":{main_bytes},"wal_bytes":{wal_bytes},"pre_us":{pre},"copy_main_us":{copy_main},"copy_wal_us":{copy_wal},"copy_open_us":{copy_open},"copy_identity_us":{copy_identity},"quick_check_us":{quick_check},"cleanup_us":{cleanup},"source_open_us":{source_open},"total_us":{}"#,
        ops::micros(total)
    )
}

/// Contention writer: 8 x 1 KiB mutations per commit until the stop file appears.
/// Stores above the existing size cap cannot be opened by the existing writer path;
/// there the same mutations run on a raw read-write connection (no `io.lock`).
pub fn writer(root: &Path) {
    let store = Store::open(root, super::data::limits(), ops::deadline(), lease_port());
    let raw = store.is_err().then(|| {
        let conn = Connection::open(root.join("store.sqlite3")).unwrap();
        no_checkpoints(&conn);
        conn.pragma_update(None, "synchronous", "FULL").unwrap();
        conn
    });
    let stop = std::path::PathBuf::from(run::env("HF_MEASURE_STOP"));
    let checkpoint = run::env("HF_MEASURE_CKPT") == "1";
    let (mut latencies, mut errors, mut checkpoints, mut n) = (Vec::new(), 0u64, 0u64, 0u64);
    let started = Instant::now();
    let window = std::path::PathBuf::from(format!("{}.window", stop.display()));
    let (mut window_commits, mut window_errors) = (0u64, 0u64);
    while !stop.exists() {
        n += 1;
        let in_window = window.exists();
        let step = Instant::now();
        let ok = match (&store, &raw) {
            (Ok(s), _) => s.apply_batch(&batch(n), ops::deadline()).is_ok(),
            (_, Some(c)) => raw_commit(c, n),
            _ => false,
        };
        if ok {
            latencies.push(ops::micros(step));
        } else {
            errors += 1;
        }
        if in_window {
            window_commits += u64::from(ok);
            window_errors += u64::from(!ok);
        }
        if checkpoint && n % 200 == 0 && checkpointed(&store, &raw) {
            checkpoints += 1;
        }
    }
    latencies.sort_unstable();
    let at = |p: usize| {
        latencies
            .get((latencies.len() * p / 100).saturating_sub(1))
            .copied()
            .unwrap_or(0)
    };
    println!(
        r#"HFW "mode":"{}","commits":{},"errors":{errors},"checkpoints":{checkpoints},"seconds":{:.2},"p50_us":{},"p99_us":{},"max_us":{},"window_commits":{window_commits},"window_errors":{window_errors}"#,
        if raw.is_some() { "raw" } else { "store" },
        latencies.len(),
        started.elapsed().as_secs_f64(),
        at(50),
        at(99),
        latencies.last().copied().unwrap_or(0)
    );
}

fn batch(n: u64) -> Vec<Mutation> {
    (0..8)
        .map(|k| Mutation::Put {
            key: format!("w/{}", (n * 8 + k) % 4096),
            value: vec![b'x'; 1024],
        })
        .collect()
}

fn raw_commit(conn: &Connection, n: u64) -> bool {
    let run = || -> rusqlite::Result<()> {
        let tx = conn.unchecked_transaction()?;
        for m in batch(n) {
            if let Mutation::Put { key, value } = m {
                tx.execute(
                    "INSERT OR REPLACE INTO fixture_records VALUES(?1,?2)",
                    rusqlite::params![key, value],
                )?;
            }
        }
        tx.commit()
    };
    run().is_ok()
}

fn checkpointed(store: &Result<Store, hf_storage::Error>, raw: &Option<Connection>) -> bool {
    match (store, raw) {
        (Ok(s), _) => s.checkpoint_passive(ops::deadline()).is_ok(),
        (_, Some(c)) => c
            .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(()))
            .is_ok(),
        _ => false,
    }
}

/// Keeps a read-only connection open (as a live owner would) until the stop file appears.
pub fn hold(root: &Path) {
    let candidate = ops::ro_open(root).unwrap();
    let stop = std::path::PathBuf::from(run::env("HF_MEASURE_STOP"));
    println!("HFH READY");
    while !stop.exists() {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    drop(candidate);
}

pub fn start_holder(root: &Path) -> std::process::Child {
    let stop = root.join("stop-holder");
    let _ = fs::remove_file(&stop);
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "child",
            "--ignored",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env("HF_MEASURE_OP", "hold")
        .env("HF_MEASURE_ROOT", root)
        .env("HF_MEASURE_KIND", "0")
        .env("HF_MEASURE_TASK", "0")
        .env("HF_MEASURE_STOP", &stop)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let out = child.stdout.take().unwrap();
    for line in std::io::BufRead::lines(std::io::BufReader::new(out)) {
        if line.unwrap().contains("HFH READY") {
            break;
        }
    }
    child
}

pub fn stop_holder(child: &mut std::process::Child) {
    child.kill().unwrap();
    child.wait().unwrap();
}
