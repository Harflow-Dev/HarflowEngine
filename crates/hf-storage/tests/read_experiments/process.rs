//! Experiments 3 and 5: killed readers and writer concurrency.
use super::{common::lease::lease_port, data, ops, util::*};
use hf_application::lease::LeaseMode;
use hf_storage::{Mutation, Store};
use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// Child entry point; the parent selects the role with HF_EXP_OP.
pub fn child() {
    let root = PathBuf::from(var("HF_EXP_ROOT"));
    let stop = PathBuf::from(var("HF_EXP_STOP"));
    match var("HF_EXP_OP").as_str() {
        "ro_open" | "ro_hold" => {
            println!("HFE BEGIN");
            let c = ops::ro_open(&root).unwrap();
            if var("HF_EXP_OP") == "ro_hold" {
                c.conn.execute_batch("BEGIN").unwrap();
                let _: i64 = c
                    .conn
                    .query_row("SELECT count(*) FROM task_headers", [], |r| r.get(0))
                    .unwrap();
            }
            println!("HFE READY");
            std::thread::sleep(Duration::from_secs(120));
        }
        "old_open" => {
            println!("HFE BEGIN");
            let _s = ops::old_open(&root).unwrap();
            println!("HFE READY");
            std::thread::sleep(Duration::from_secs(120));
        }
        "inv_writer" => inv_writer(&root, &stop),
        _ => inv_reader(&root, &stop),
    }
}

fn spawn(op: &str, root: &Path, stop: &Path) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "exp_child",
            "--ignored",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env("HF_EXP_OP", op)
        .env("HF_EXP_ROOT", root)
        .env("HF_EXP_STOP", stop)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// Reads child output until `marker`; the reader must stay alive until the child is
/// reaped, or the child's next print would fail on a closed pipe.
fn wait_marker(child: &mut Child, marker: &str) -> BufReader<std::process::ChildStdout> {
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        assert!(
            reader.read_line(&mut line).unwrap() > 0,
            "child ended before {marker}"
        );
        if line.contains(marker) {
            return reader;
        }
    }
}

fn wait_ready(child: &mut Child) {
    drop(wait_marker(child, "HFE READY"));
}

fn lease_free(root: &Path) -> bool {
    lease_port()
        .acquire(
            &root.join("store.lock"),
            LeaseMode::Exclusive,
            false,
            ops::deadline(),
        )
        .is_ok()
}

fn readable(root: &Path) -> String {
    match ops::ro_open(root) {
        Ok(c) => counts(&c.conn),
        Err(e) => format!("ERROR {e}"),
    }
}

fn kill(child: &mut Child) {
    child.kill().unwrap();
    child.wait().unwrap();
}

pub fn killed() {
    let mut file = out("killed.jsonl");
    let work = fresh("kill-hold");
    let root = work.join("store");
    cp(&dir().join("ci-wal"), &root);
    let mut holder = spawn("ro_hold", &root, &root);
    wait_ready(&mut holder);
    let held_busy = !lease_free(&root);
    let during = names(&root);
    kill(&mut holder);
    // Remnants are captured immediately after the reap, before any reopen.
    let after_kill = names(&root);
    let parent_after = names(&work);
    let free_after = lease_free(&root).to_string();
    record(
        &mut file,
        &[
            ("case", "ro_hold_midtransaction".into()),
            ("mode", "after_ready".into()),
            ("exclusive_lease_busy_while_held", held_busy.to_string()),
            ("files_during", during),
            ("files_after_reap", after_kill),
            ("parent_dir_after_reap", parent_after),
            ("exclusive_lease_free_after", free_after),
            ("next_read", readable(&root)),
        ],
    );
    let spawn_relative = |base: &'static str,
                          op: &'static str,
                          step: u64,
                          n: u64|
     -> Vec<(&'static str, &'static str, &'static str, u64)> {
        (1..=n)
            .map(|i| (base, op, "spawn_relative", i * step))
            .collect()
    };
    let staged = |base: &'static str,
                  op: &'static str,
                  mode: &'static str,
                  delays: &[u64]|
     -> Vec<(&'static str, &'static str, &'static str, u64)> {
        delays.iter().map(|d| (base, op, mode, *d)).collect()
    };
    let mut trials = Vec::new();
    trials.extend(staged("ci-wal-noshm", "ro_open", "after_ready", &[0; 5]));
    trials.extend(staged(
        "ci-wal-noshm",
        "ro_open",
        "after_begin",
        &[0, 1, 2, 3, 5, 8],
    ));
    trials.extend(staged(
        "large-wal",
        "ro_open",
        "after_begin",
        &[0, 10, 20, 40, 80, 120, 160],
    ));
    trials.extend(staged(
        "ci-wal",
        "old_open",
        "after_begin",
        &(1..=30).map(|i| i * 2).collect::<Vec<_>>(),
    ));
    trials.extend(spawn_relative("large-wal", "ro_open", 12, 14));
    trials.extend(spawn_relative("ci-wal", "old_open", 2, 30));
    for (base, op, mode, d) in trials {
        kill_trial(&mut file, base, op, mode, d);
    }
}

fn kill_trial(file: &mut std::fs::File, base: &str, op: &str, mode: &str, d: u64) {
    let work = fresh("kill-open");
    let root = work.join("store");
    cp(&dir().join(base.trim_end_matches("-noshm")), &root);
    if base.ends_with("-noshm") {
        std::fs::remove_file(root.join("store.sqlite3-shm")).unwrap();
    }
    let before = names(&root);
    let mut c = spawn(op, &root, &root);
    let reader = match mode {
        "after_ready" => Some(wait_marker(&mut c, "HFE READY")),
        "after_begin" => {
            let r = wait_marker(&mut c, "HFE BEGIN");
            std::thread::sleep(Duration::from_millis(d));
            Some(r)
        }
        _ => {
            std::thread::sleep(Duration::from_millis(d));
            None
        }
    };
    let exited = c.try_wait().unwrap().is_some();
    kill(&mut c);
    drop(reader);
    // Captured before the verification read below, which may itself create `-shm`.
    let new_files = diff_names(&before, &names(&root));
    let parent_after = names(&work);
    let lease = lease_free(&root).to_string();
    let next = Instant::now();
    let content = readable(&root);
    record(
        file,
        &[
            ("case", format!("{op}_killed")),
            ("mode", mode.into()),
            ("base", base.into()),
            ("delay_ms", d.to_string()),
            ("exited_before_kill", exited.to_string()),
            ("root_files_new_at_reap", new_files),
            ("parent_dir_after_reap", parent_after),
            ("exclusive_lease_free_after", lease),
            ("next_read", content),
            ("next_read_ms", next.elapsed().as_millis().to_string()),
        ],
    );
}

fn diff_names(before: &str, after: &str) -> String {
    let b: Vec<&str> = before.split(',').collect();
    after
        .split(',')
        .filter(|n| !b.contains(n))
        .collect::<Vec<_>>()
        .join(",")
}

fn inv_writer(root: &Path, stop: &Path) {
    let store = Store::open(root, data::limits(), ops::deadline(), lease_port()).unwrap();
    let (mut n, mut errors, mut complete, mut partial) = (0u64, 0u64, 0u64, 0u64);
    while !stop.exists() {
        n += 1;
        let v = n.to_string().into_bytes();
        let batch = [
            Mutation::Put {
                key: "inv/a".into(),
                value: v.clone(),
            },
            Mutation::Put {
                key: "inv/b".into(),
                value: v,
            },
        ];
        errors += u64::from(store.apply_batch(&batch, ops::deadline()).is_err());
        if n % 50 == 0 {
            match store.checkpoint_passive(ops::deadline()) {
                Ok(c) if c.complete => complete += 1,
                _ => partial += 1,
            }
        }
    }
    println!(
        "HFE writer commits={n} errors={errors} ckpt_complete={complete} ckpt_incomplete={partial}"
    );
}

fn inv_reader(root: &Path, stop: &Path) {
    let c = ops::ro_open(root).unwrap();
    let (mut reads, mut torn, mut errors, mut last) = (0u64, 0u64, 0u64, 0u64);
    let mut backwards = 0u64;
    while !stop.exists() {
        let r: rusqlite::Result<(Vec<u8>, Vec<u8>)> = (|| {
            c.conn.execute_batch("BEGIN")?;
            let a = c.conn.query_row(
                "SELECT value FROM fixture_records WHERE key='inv/a'",
                [],
                |r| r.get(0),
            );
            let b = c.conn.query_row(
                "SELECT value FROM fixture_records WHERE key='inv/b'",
                [],
                |r| r.get(0),
            );
            c.conn.execute_batch("COMMIT")?;
            Ok((a?, b?))
        })();
        match r {
            Ok((a, b)) => {
                reads += 1;
                torn += u64::from(a != b);
                let v: u64 = String::from_utf8_lossy(&a).parse().unwrap_or(0);
                backwards += u64::from(v < last);
                last = last.max(v);
            }
            Err(_) => {
                errors += 1;
                let _ = c.conn.execute_batch("ROLLBACK");
            }
        }
    }
    println!("HFE reader reads={reads} torn={torn} errors={errors} went_backwards={backwards}");
}

pub fn writer_safety() {
    let work = fresh("safety");
    let root = work.join("store");
    cp(&dir().join("ci-wal"), &root);
    let stop = work.join("stop");
    let mut writer = spawn("inv_writer", &root, &stop);
    std::thread::sleep(Duration::from_secs(1));
    // The writer's first commit creates the keys before readers start looking.
    let mut readers: Vec<Child> = (0..4).map(|_| spawn("inv_reader", &root, &stop)).collect();
    std::thread::sleep(Duration::from_secs(10));
    std::fs::write(&stop, b"x").unwrap();
    let mut file = out("safety.jsonl");
    let collect = |c: &mut Child| {
        let mut text = String::new();
        let out = c.stdout.take().unwrap();
        for l in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(at) = l.find("HFE ") {
                text = l[at + 4..].to_owned();
            }
        }
        c.wait().unwrap();
        text
    };
    record(&mut file, &[("role", collect(&mut writer))]);
    for r in &mut readers {
        record(&mut file, &[("role", collect(r))]);
    }
    let mut holder = spawn("ro_hold", &root, &stop);
    wait_ready(&mut holder);
    let refused = Store::recover(&root, ops::deadline(), lease_port());
    kill(&mut holder);
    let after = Store::recover(&root, ops::deadline(), lease_port());
    record(
        &mut file,
        &[
            ("recover_while_reader_holds_lease", format!("{refused:?}")),
            ("recover_after_reader_killed", format!("{after:?}")),
            ("files_after", names(&root)),
        ],
    );
}
