//! Child-mode write latency: the parent re-executes this test binary once per operation;
//! the child opens a writer with the measured options, performs the operation and exits.
//! Also the in-process series and the four-writer contention run.
use super::support::{
    self, Children, WorkDir, batch, copy_tree, env, events, micros, number, open, out_file,
    outcome, retry, task,
};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// How long a contending child waits for the start file before giving up.
const GO_DEADLINE: Duration = Duration::from_secs(30);

/// One child-mode series: operation shape, durability mode and checkpoint policy.
#[derive(Clone, Debug)]
pub struct Series {
    pub shape: String,
    pub mode: String,
    pub policy: String,
}
impl Series {
    pub fn parse(text: &str) -> Self {
        let mut parts = text.split('/');
        let mut next = || parts.next().unwrap().to_owned();
        Self {
            shape: next(),
            mode: next(),
            policy: next(),
        }
    }
    pub fn name(&self) -> String {
        format!("{}-{}-{}", self.shape, self.mode, self.policy)
    }
}

/// The child side of one operation. Prints `HFW open_us op_us busy err`.
pub fn child_operation() {
    let root = PathBuf::from(env("HF_WL_ROOT"));
    let (mode, policy, name) = (env("HF_WL_MODE"), env("HF_WL_POLICY"), env("HF_WL_NAME"));
    let mut busy = 0;
    let started = Instant::now();
    let store = open(&root, &mode, &policy, &mut busy);
    let open_us = micros(started);
    let started = Instant::now();
    let result = store.and_then(|store| {
        retry(&mut busy, || {
            store.create_task(&task(&name), support::later(10))
        })?;
        if env("HF_WL_SHAPE") == "create_append" {
            let batch = events(1, 8, 1024);
            let id = support::task_id(&name);
            retry(&mut busy, || {
                store.append_events(&id, &batch, &support::facts(), support::later(10))
            })?;
        }
        Ok(())
    });
    println!(
        "HFW {open_us} {} {busy} {}",
        micros(started),
        outcome(&result)
    );
}

fn spawn_operation(series: &Series, root: &Path, name: &str) -> (u64, String) {
    let started = Instant::now();
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "child",
            "--ignored",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env("HF_WL_OP", "operation")
        .env("HF_WL_ROOT", root)
        .env("HF_WL_MODE", &series.mode)
        .env("HF_WL_POLICY", &series.policy)
        .env("HF_WL_SHAPE", &series.shape)
        .env("HF_WL_NAME", name)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .unwrap();
    let wall = micros(started);
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find_map(|l| l.find("HFW ").map(|at| l[at + 4..].to_owned()))
        .unwrap_or_else(|| "0 0 0 ChildFailed".into());
    (wall, line)
}

/// Interleaved child-mode series, round-robin with a rotating start, each on its own copy
/// of the dataset root. Writes one raw line per sample, warm-up included and marked.
pub fn run_child_series(label: &str, series: &[Series]) {
    let (warm, kept) = (number("HF_MEASURE_WARM", 50), number("HF_MEASURE_N", 500));
    let work = WorkDir::new(&format!("wl-{label}"));
    std::fs::create_dir_all(&work.0).unwrap();
    let source = support::dir().join(support::env_or("HF_MEASURE_CASE", "ci-ckpt"));
    let roots: Vec<PathBuf> = series
        .iter()
        .map(|s| {
            let root = work.0.join(s.name());
            copy_tree(&source, &root);
            root
        })
        .collect();
    let mut file = out_file(&format!("{label}-child.jsonl"));
    // `HF_WL_RESET=n` re-clones every root after each n rounds (untimed), so a series that
    // never checkpoints keeps a bounded `-wal`; 0 lets it grow for the whole run. The
    // default, 50, is the A7b gate definition.
    let reset = number("HF_WL_RESET", 50);
    batch(&format!("{label}-child"), "start");
    for i in 0..warm + kept {
        if reset > 0 && i > 0 && i % reset == 0 {
            roots.iter().for_each(|root| copy_tree(&source, root));
        }
        for k in 0..series.len() {
            let at = (k + i as usize) % series.len();
            let s = &series[at];
            let (wall, line) = spawn_operation(s, &roots[at], &format!("wl-{i:06}"));
            let mut parts = line.split(' ');
            let mut field = || parts.next().unwrap_or("").to_owned();
            let (open_us, op_us, busy, err) = (field(), field(), field(), field());
            writeln!(file, r#"{{"series":"{}","i":{i},"warm":{},"wall_us":{wall},"open_us":{open_us},"op_us":{op_us},"busy":{busy},"err":"{err}"}}"#, s.name(), i < warm).unwrap();
        }
    }
    batch(&format!("{label}-child"), "end");
    record_wal(label, series, &roots);
}

fn record_wal(label: &str, series: &[Series], roots: &[PathBuf]) {
    let mut file = out_file("roots.jsonl");
    for (s, root) in series.iter().zip(roots) {
        let size = |name: &str| std::fs::metadata(root.join(name)).map_or(0, |m| m.len());
        writeln!(
            file,
            r#"{{"run":"{label}","series":"{}","main":{},"wal":{}}}"#,
            s.name(),
            size("store.sqlite3"),
            size("store.sqlite3-wal")
        )
        .unwrap();
    }
}

/// One long-lived Suppressed handle per mode, round-robin, one `create_task` per sample.
pub fn run_in_process(label: &str, modes: &[&str]) {
    let (warm, kept) = (number("HF_MEASURE_WARM", 50), number("HF_MEASURE_N", 500));
    let work = WorkDir::new(&format!("wl-{label}-inproc"));
    std::fs::create_dir_all(&work.0).unwrap();
    let source = support::dir().join(support::env_or("HF_MEASURE_CASE", "ci-ckpt"));
    let mut busy = 0;
    let stores: Vec<_> = modes
        .iter()
        .map(|mode| {
            let root = work.0.join(mode);
            copy_tree(&source, &root);
            open(&root, mode, "suppressed", &mut busy).unwrap()
        })
        .collect();
    let mut file = out_file(&format!("{label}-inproc.jsonl"));
    batch(&format!("{label}-inproc"), "start");
    for i in 0..warm + kept {
        for (mode, store) in modes.iter().zip(&stores) {
            let started = Instant::now();
            let result = retry(&mut busy, || {
                store.create_task(&task(&format!("ip-{i:06}")), support::later(10))
            });
            let us = micros(started);
            writeln!(file, r#"{{"series":"inproc-create-{mode}-suppressed","i":{i},"warm":{},"wall_us":{us},"busy":{busy},"err":"{}"}}"#, i < warm, outcome(&result)).unwrap();
        }
    }
    batch(&format!("{label}-inproc"), "end");
    drop(stores);
}

/// One contending writer: opens, creates its task, waits for the start file, then commits
/// `HF_WL_COMMITS` appends of about 200 KB. Prints `HFC us busy err` per commit.
pub fn child_contend() {
    let root = PathBuf::from(env("HF_WL_ROOT"));
    let (policy, name) = (env("HF_WL_POLICY"), env("HF_WL_NAME"));
    let go = PathBuf::from(env("HF_WL_GO"));
    let mut busy = 0;
    let store = open(&root, &env("HF_WL_MODE"), &policy, &mut busy).unwrap();
    retry(&mut busy, || {
        store.create_task(&task(&name), support::later(10))
    })
    .unwrap();
    println!("HFR ready");
    let waiting = Instant::now();
    while !go.exists() {
        if waiting.elapsed() > GO_DEADLINE {
            eprintln!("contending writer {name}: no start file after {GO_DEADLINE:?}");
            std::process::exit(2);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let id = support::task_id(&name);
    for commit in 0..number("HF_WL_COMMITS", 100) {
        let batch = events(commit * 13 + 1, 13, 15_750);
        let mut busy = 0;
        let started = Instant::now();
        let result = retry(&mut busy, || {
            store.append_events(&id, &batch, &support::facts(), support::later(10))
        });
        println!("HFC {} {busy} {}", micros(started), outcome(&result));
    }
}

fn spawn_contender(
    w: usize,
    work: &Path,
    go: &Path,
    mode: &str,
    policy: &str,
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
        .env("HF_WL_OP", "contend")
        .env("HF_WL_ROOT", work)
        .env("HF_WL_MODE", mode)
        .env("HF_WL_POLICY", policy)
        .env("HF_WL_NAME", format!("cw-{w}"))
        .env("HF_WL_GO", go)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap()
}

/// Reads one contender's samples, waits for it, and checks that it exited successfully
/// with one sample per commit.
fn collect_contender(
    child: &mut std::process::Child,
    w: usize,
    name: &str,
    file: &mut std::fs::File,
) {
    let mut text = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        stdout.read_to_string(&mut text).unwrap();
    }
    let status = child.wait().unwrap();
    assert!(
        status.success(),
        "contending writer {w} exited with {status}"
    );
    let samples: Vec<&str> = text
        .lines()
        .filter_map(|l| l.find("HFC ").map(|at| &l[at + 4..]))
        .collect();
    let expected = number("HF_WL_COMMITS", 100);
    assert_eq!(
        samples.len() as u64,
        expected,
        "contending writer {w} samples"
    );
    for (k, line) in samples.into_iter().enumerate() {
        let mut parts = line.split(' ');
        let (us, busy) = (parts.next().unwrap_or("0"), parts.next().unwrap_or("0"));
        let err = parts.next().unwrap_or("");
        writeln!(file, r#"{{"series":"{name}","writer":{w},"k":{k},"wall_us":{us},"busy":{busy},"err":"{err}"}}"#).unwrap();
    }
}

/// Four writer processes on one root, each committing 200 KB appends at full speed.
pub fn run_contention(label: &str, mode: &str, policy: &str) {
    let work = WorkDir::new(&format!("wl-{label}-contend-{policy}"));
    let source = support::dir().join(support::env_or("HF_MEASURE_CASE", "ci-ckpt"));
    copy_tree(&source, &work.0);
    let go = WorkDir::new(&format!("wl-{label}-go"));
    let _ = std::fs::remove_file(&go.0);
    let name = format!("{label}-contend-{mode}-{policy}");
    batch(&name, "start");
    let mut children = Children(Vec::new());
    for w in 0..4 {
        children
            .0
            .push(spawn_contender(w, &work.0, &go.0, mode, policy));
    }
    std::thread::sleep(Duration::from_secs(3));
    std::fs::write(&go.0, b"go").unwrap();
    let started = Instant::now();
    let mut file = out_file(&format!("{name}.jsonl"));
    for (w, child) in children.0.iter_mut().enumerate() {
        collect_contender(child, w, &name, &mut file);
    }
    let seconds = started.elapsed().as_secs_f64();
    let wal = std::fs::metadata(work.0.join("store.sqlite3-wal")).map_or(0, |m| m.len());
    writeln!(
        out_file("roots.jsonl"),
        r#"{{"run":"{name}","seconds":{seconds:.2},"wal":{wal}}}"#
    )
    .unwrap();
    batch(&name, "end");
}
