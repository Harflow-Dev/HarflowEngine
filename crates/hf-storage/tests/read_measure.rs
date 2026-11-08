#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]
//! Slice S2 measurement harness: deterministic dataset generation and read-path
//! timing. Every test is `#[ignore]`; see docs/store-read-measurement.md.
mod common;
#[path = "read_measure/components.rs"]
mod components;
#[path = "read_measure/data.rs"]
mod data;
#[path = "read_measure/ops.rs"]
mod ops;
#[path = "read_measure/prompt.rs"]
mod prompt;
#[path = "read_measure/run.rs"]
mod run;

use run::{env, env_or};
use std::{io::Write, path::PathBuf, process::Command, time::Instant};

fn dir() -> PathBuf {
    PathBuf::from(env("HF_MEASURE_DIR"))
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    let status = Command::new("cp")
        .arg("-cR")
        .arg(from)
        .arg(to)
        .status()
        .unwrap();
    assert!(status.success());
}

fn truncate_checkpoint(root: &std::path::Path) {
    let conn = rusqlite::Connection::open(root.join("store.sqlite3")).unwrap();
    conn.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
        true,
    )
    .unwrap();
    let (busy, log, done): (i64, i64, i64) = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!((busy, log, done), (0, log, log));
}

fn size(path: PathBuf) -> u64 {
    std::fs::metadata(path).map_or(0, |m| m.len())
}

#[test]
#[ignore]
fn generate() {
    let name = env("HF_MEASURE_DATASET");
    let (tasks, per_task) = match name.as_str() {
        "large" => (5000, 100),
        // The 10,000-task statistics world: same generator, 10 events per task.
        "stats10k" => (10_000, 10),
        _ => (300, 100),
    };
    // HF_MEASURE_TASKBYTES=legacy reproduces the S2 task content; the default is the S6 content.
    let bytes = match run::env_or("HF_MEASURE_TASKBYTES", "valid").as_str() {
        "legacy" => data::TaskBytes::Legacy,
        _ => data::TaskBytes::Valid,
    };
    let wal = dir().join(format!("{name}-wal"));
    let elapsed = data::generate(
        &wal,
        tasks,
        per_task,
        None,
        bytes,
        common::lease::lease_port(),
    );
    let ckpt = dir().join(format!("{name}-ckpt"));
    copy_tree(&wal, &ckpt);
    truncate_checkpoint(&ckpt);
    let mut file = run::out_file("generate.jsonl");
    writeln!(
        file,
        r#"{{"dataset":"{name}","tasks":{tasks},"events":{},"generate_s":{:.1},"wal_main":{},"wal_wal":{},"ckpt_main":{},"ckpt_wal":{}}}"#,
        tasks * per_task,
        elapsed.as_secs_f64(),
        size(wal.join("store.sqlite3")),
        size(wal.join("store.sqlite3-wal")),
        size(ckpt.join("store.sqlite3")),
        size(ckpt.join("store.sqlite3-wal"))
    )
    .unwrap();
}

/// One valid task whose prompt is 4 MiB (`HF_MEASURE_DATASET` names the directory
/// under `HF_MEASURE_DIR`), for the `task-prompt` component case.
#[test]
#[ignore]
fn generate_prompt() {
    let form = env_or("HF_MEASURE_PROMPT_FORM", "sidecar");
    prompt::generate(&dir().join(env("HF_MEASURE_DATASET")), form == "inline");
}

#[test]
#[ignore]
fn child() {
    let root = PathBuf::from(env("HF_MEASURE_ROOT"));
    let kind: u64 = env("HF_MEASURE_KIND").parse().unwrap();
    let task = data::task_id(env("HF_MEASURE_TASK").parse().unwrap());
    let timing = match env("HF_MEASURE_OP").as_str() {
        "old" => ops::old_once(&root, kind, &task),
        "ro" => ops::ro_once(&root, kind, &task),
        "writer" => return components::writer(&root),
        "hold" => return components::hold(&root),
        "sqlite_version" => return println!("HFS {}", rusqlite::version()),
        _ => ops::Timing {
            open_us: 0,
            query_us: 0,
            err: String::new(),
        },
    };
    println!("HFM {} {} {}", timing.open_us, timing.query_us, timing.err);
}

fn cold_batch(path: &str, case: &str, n: u64, label: &str, extra: &[(&str, String)]) {
    let root = dir().join(case);
    let mut file = run::out_file(&format!("{label}-{case}-{path}.jsonl"));
    for i in 0..n {
        let (kind, task) = run::draw(case, i);
        let child = run::spawn(path, &root, kind, task, extra);
        run::sample_line(&mut file, case, path, i, kind, &child);
    }
}

#[test]
#[ignore]
fn cold() {
    let (path, label) = (env("HF_MEASURE_PATH"), env("HF_MEASURE_LABEL"));
    let n: u64 = env_or("HF_MEASURE_N", "200").parse().unwrap();
    for case in env("HF_MEASURE_CASES").split(',') {
        let holder = env_or("HF_MEASURE_HOLDER", "0") == "1";
        let label = if holder {
            format!("{label}H")
        } else {
            label.clone()
        };
        run::batch(&format!("{label}-{case}-{path}"), "start");
        let mut held = holder.then(|| components::start_holder(&dir().join(case)));
        cold_batch(&path, case, n, &label, &[]);
        if let Some(h) = held.as_mut() {
            components::stop_holder(h);
        }
        run::batch(&format!("{label}-{case}-{path}"), "end");
    }
}

#[test]
#[ignore]
fn warm() {
    let (path, label) = (env("HF_MEASURE_PATH"), env("HF_MEASURE_LABEL"));
    for case in env("HF_MEASURE_CASES").split(',') {
        let root = dir().join(case);
        let mut file = run::out_file(&format!("{label}-{case}-warm-{path}.jsonl"));
        run::batch(&format!("{label}-{case}-warm-{path}"), "start");
        let store = if path == "old" {
            Some(ops::old_open(&root))
        } else {
            None
        };
        let candidate = if path == "ro" {
            Some(ops::ro_open(&root))
        } else {
            None
        };
        for i in 0..1020u64 {
            let (kind, task) = run::draw(case, i);
            let task = data::task_id(task);
            let started = Instant::now();
            let result = match (&store, &candidate) {
                (Some(Ok(s)), _) => ops::old_query(s, kind, &task),
                (_, Some(Ok(c))) => ops::ro_query(c, kind, &task),
                _ => Err("OpenFailed".into()),
            };
            let us = ops::micros(started);
            if i >= 20 {
                let err = result.err().unwrap_or_default();
                writeln!(file, r#"{{"case":"{case}","path":"warm-{path}","i":{i},"kind":{kind},"wall_us":{us},"open_us":0,"query_us":{us},"err":"{err}"}}"#).unwrap();
            }
        }
        run::batch(&format!("{label}-{case}-warm-{path}"), "end");
    }
}

#[test]
#[ignore]
fn null() {
    let label = env("HF_MEASURE_LABEL");
    let n: u64 = env_or("HF_MEASURE_N", "200").parse().unwrap();
    run::batch(&format!("{label}-null"), "start");
    let mut file = run::out_file(&format!("{label}-null.jsonl"));
    for i in 0..n {
        let child = run::spawn("null", &dir(), 0, 0, &[]);
        run::sample_line(&mut file, "none", "null", i, 0, &child);
    }
    run::batch(&format!("{label}-null"), "end");
}

fn start_writer(
    scratch: &std::path::Path,
    stop: &std::path::Path,
    checkpoint: &str,
) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "child",
            "--ignored",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env("HF_MEASURE_OP", "writer")
        .env("HF_MEASURE_ROOT", scratch)
        .env("HF_MEASURE_KIND", "0")
        .env("HF_MEASURE_TASK", "0")
        .env("HF_MEASURE_STOP", stop)
        .env("HF_MEASURE_CKPT", checkpoint)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}

#[test]
#[ignore]
fn contended() {
    let (path, label) = (env("HF_MEASURE_PATH"), env("HF_MEASURE_LABEL"));
    let source = env("HF_MEASURE_CASE");
    let checkpoint = env_or("HF_MEASURE_CKPT", "0");
    let scratch = dir().join(format!("{source}-contended"));
    let _ = std::fs::remove_dir_all(&scratch);
    copy_tree(&dir().join(&source), &scratch);
    let tag = if env_or("HF_MEASURE_NOWRITER", "0") == "1" {
        "-nowriter"
    } else {
        ""
    };
    let name = format!("{label}-{source}-{path}-contended{tag}-ckpt{checkpoint}");
    let stop = dir().join("stop-writer");
    let _ = std::fs::remove_file(&stop);
    run::batch(&name, "start");
    let no_writer = env_or("HF_MEASURE_NOWRITER", "0") == "1";
    let per: u64 = env_or("HF_MEASURE_CN", "50").parse().unwrap();
    let window = dir().join("stop-writer.window");
    let _ = std::fs::remove_file(&window);
    let writer = (!no_writer).then(|| start_writer(&scratch, &stop, &checkpoint));
    std::thread::sleep(std::time::Duration::from_secs(if no_writer {
        0
    } else {
        2
    }));
    let began = Instant::now();
    std::fs::write(&window, b"x").unwrap();
    let readers: Vec<_> = (0..4u64)
        .map(|r| {
            let (path, case, scratch) = (path.clone(), source.clone(), scratch.clone());
            let file = format!("{name}-r{r}.jsonl");
            std::thread::spawn(move || {
                let mut out = run::out_file(&file);
                for i in 0..per {
                    let (kind, task) = run::draw(&case, r * per + i);
                    let child = run::spawn(&path, &scratch, kind, task, &[]);
                    run::sample_line(&mut out, &case, &path, r * per + i, kind, &child);
                }
            })
        })
        .collect();
    readers.into_iter().for_each(|t| t.join().unwrap());
    let reader_window_s = began.elapsed().as_secs_f64();
    let _ = std::fs::remove_file(&window);
    std::fs::write(&stop, b"x").unwrap();
    let mut file = run::out_file("writer.jsonl");
    if let Some(writer) = writer {
        let output = writer.wait_with_output().unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text
            .lines()
            .filter_map(|l| l.find("HFW ").map(|at| &l[at + 4..]))
        {
            writeln!(
                file,
                r#"{{"run":"{name}","reader_window_s":{reader_window_s:.2},{line}}}"#
            )
            .unwrap();
        }
    } else {
        writeln!(file, r#"{{"run":"{name}","reader_window_s":{reader_window_s:.2},"mode":"none","commits":0,"errors":0,"checkpoints":0,"seconds":0,"p50_us":0,"p99_us":0,"max_us":0,"window_commits":0,"window_errors":0}}"#).unwrap();
    }
    run::batch(&name, "end");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
#[ignore]
fn components() {
    let label = env("HF_MEASURE_LABEL");
    let n: u64 = env_or("HF_MEASURE_N", "30").parse().unwrap();
    for case in env("HF_MEASURE_CASES").split(',') {
        run::batch(&format!("{label}-{case}-components"), "start");
        let work = dir().join(format!("comp-{case}"));
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir(&work).unwrap();
        copy_tree(&dir().join(case), &work.join("store"));
        let mut file = run::out_file(&format!("{label}-{case}-components.jsonl"));
        for i in 0..n {
            let line = components::replay(&work.join("store"), &work);
            writeln!(file, r#"{{"case":"{case}","i":{i},{line}}}"#).unwrap();
        }
        let _ = std::fs::remove_dir_all(&work);
        run::batch(&format!("{label}-{case}-components"), "end");
    }
}

/// Derives `<name>-tail`: the checkpointed store plus a small post-checkpoint WAL,
/// the steady state a checkpoint owner would maintain. Raw SQL writes, deviation
/// noted in the protocol: the existing writer cannot open a store above the cap.
#[test]
#[ignore]
fn tail() {
    let name = env("HF_MEASURE_DATASET");
    let target = dir().join(format!("{name}-tail"));
    copy_tree(&dir().join(format!("{name}-ckpt")), &target);
    let conn = rusqlite::Connection::open(target.join("store.sqlite3")).unwrap();
    conn.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
        true,
    )
    .unwrap();
    conn.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    conn.pragma_update(None, "synchronous", "FULL").unwrap();
    for n in 0..200u64 {
        let tx = conn.unchecked_transaction().unwrap();
        for k in 0..8u64 {
            tx.execute(
                "INSERT OR REPLACE INTO fixture_records VALUES(?1,?2)",
                rusqlite::params![format!("w/{}", (n * 8 + k) % 4096), vec![b'x'; 1024]],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    drop(conn);
    let mut file = run::out_file("generate.jsonl");
    writeln!(
        file,
        r#"{{"dataset":"{name}-tail","wal":{}}}"#,
        size(target.join("store.sqlite3-wal"))
    )
    .unwrap();
}
