//! Shared pieces of the write measurement harness: environment, raw output, load
//! recording, writer handles with the measured durability, and the written rows.
use hf_domain::event_read::{CaptureState, LogFacts};
use hf_domain::task_read::TaskId;
use hf_platform::durability::{DEFAULT_DURABILITY, Durability, port};
use hf_storage::{Error, EventAppend, Limits, Store, TaskCreate, TaskFacts, WriterOptions};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

/// Pause between attempts after `Busy` (the `record_lock` policy).
pub const BUSY_PAUSE: Duration = Duration::from_millis(5);

pub fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name}"))
}

pub fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

pub fn number(name: &str, default: u64) -> u64 {
    env_or(name, &default.to_string()).parse().unwrap()
}

pub fn dir() -> PathBuf {
    PathBuf::from(env("HF_MEASURE_DIR"))
}

pub fn out_file(name: &str) -> File {
    let dir = PathBuf::from(env("HF_MEASURE_OUT"));
    std::fs::create_dir_all(&dir).unwrap();
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(name))
        .unwrap()
}

pub fn load() -> String {
    let out = Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .replace(['{', '}'], "")
        .trim()
        .to_owned()
}

/// One `batches.jsonl` line with the load average at a run boundary.
pub fn batch(label: &str, phase: &str) {
    let mut file = out_file("batches.jsonl");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    writeln!(
        file,
        r#"{{"label":"{label}","phase":"{phase}","unix":{now},"load":"{}"}}"#,
        load()
    )
    .unwrap();
}

/// A work path under `HF_MEASURE_DIR` (a cloned root or a start file), removed on drop,
/// also when the run fails.
pub struct WorkDir(pub PathBuf);
impl WorkDir {
    pub fn new(name: &str) -> Self {
        Self(dir().join(name))
    }
}
impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Child processes killed and reaped on drop, so a failing parent leaves none behind.
pub struct Children(pub Vec<std::process::Child>);
impl Drop for Children {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn copy_tree(from: &Path, to: &Path) {
    let _ = std::fs::remove_dir_all(to);
    let status = Command::new("cp")
        .arg("-cR")
        .arg(from)
        .arg(to)
        .status()
        .unwrap();
    assert!(status.success());
}

pub fn micros(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap()
}

pub fn later(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds)
}

pub fn limits() -> Limits {
    Limits {
        max_rows: 1_000_000,
        max_bytes: 256 * 1024 * 1024,
    }
}

/// The measured writer configuration: `fsync`, `full` or `default` (the production
/// `DEFAULT_DURABILITY`), and `suppressed` or `owner`.
pub fn options(mode: &str, policy: &str) -> WriterOptions {
    let base = match policy {
        "owner" => WriterOptions::owner(),
        "suppressed" => WriterOptions::suppressed(),
        other => panic!("policy {other}"),
    };
    match mode {
        "fsync" => base.with_flush(port(Durability::FsyncOnly)),
        "full" => base.with_flush(port(Durability::FullFlush)),
        "default" => base.with_flush(port(DEFAULT_DURABILITY)),
        other => panic!("mode {other}"),
    }
}

/// Runs `action` until it returns something other than `Busy`, pausing `BUSY_PAUSE`
/// between attempts, for at most ten seconds. Counts the `Busy` results in `busy`.
pub fn retry<T>(busy: &mut u64, mut action: impl FnMut() -> Result<T, Error>) -> Result<T, Error> {
    let until = later(10);
    loop {
        match action() {
            Err(Error::Busy) if Instant::now() < until => {
                *busy += 1;
                std::thread::sleep(BUSY_PAUSE);
            }
            other => return other,
        }
    }
}

pub fn open(root: &Path, mode: &str, policy: &str, busy: &mut u64) -> Result<Store, Error> {
    retry(busy, || {
        Store::open_with(
            root,
            limits(),
            later(10),
            hf_platform::lease::shared(),
            options(mode, policy),
        )
    })
}

pub fn task_id(name: &str) -> TaskId {
    TaskId::new(name.to_owned()).unwrap()
}

/// A running task with served header and status documents of typical size.
pub fn task(name: &str) -> TaskCreate {
    let id = task_id(name);
    let started = "2026-11-14T00:00:00+00:00";
    let header = format!(
        r#"{{"task_id":"{name}","backend":"claude","session_id":"s1","started_at":"{started}","pad":"{}"}}"#,
        "h".repeat(200)
    );
    let status = format!(r#"{{"task_id":"{name}","status":"running"}}"#);
    TaskCreate {
        facts: TaskFacts {
            id,
            backend: "claude".into(),
            repo_path: "/workspace/repo".into(),
            session_id: "s1".into(),
            status: "running".into(),
            exit_code: None,
            started_at: started.into(),
            started_at_us: 1_763_078_400_000_000,
            finished_at: None,
            finished_at_us: None,
            output_drained: false,
            title: None,
            model: None,
            continuation_of: None,
            chain_root: None,
        },
        header: header.into_bytes(),
        status: status.into_bytes(),
        prompt: "Summarize the repository.".into(),
        process: None,
    }
}

pub fn facts() -> LogFacts {
    LogFacts {
        task_status: Some("running".into()),
        available: true,
        capture: CaptureState::Live,
        output_drained: Some(false),
        eof_finished_seq: None,
        within_grace: true,
    }
}

/// One event line: `raw_len` is the raw line length; the stored body is the line cut to
/// the body limit at a character boundary.
pub fn event(seq: u64, line: &str) -> EventAppend {
    let mut end = line.len().min(hf_storage::EVENT_BODY_LIMIT);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    EventAppend {
        seq,
        kind: "assistant".into(),
        raw_len: line.len().max(1) as u64,
        body: line.as_bytes()[..end].to_vec(),
    }
}

/// `count` lines of `size` bytes each, numbered from `first`.
pub fn events(first: u64, count: u64, size: usize) -> Vec<EventAppend> {
    (0..count)
        .map(|n| {
            let head = format!(r#"{{"type":"assistant","seq":{},"text":""#, first + n);
            let pad = size.saturating_sub(head.len() + 2);
            event(first + n, &format!("{head}{}\"}}", "x".repeat(pad)))
        })
        .collect()
}

/// The error name for a raw line; empty on success.
pub fn outcome<T>(result: &Result<T, Error>) -> String {
    match result {
        Ok(_) => String::new(),
        Err(error) => format!("{error:?}")
            .chars()
            .take(60)
            .collect::<String>()
            .replace(['"', ' '], "_"),
    }
}
