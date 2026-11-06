//! Process spawning, raw-sample output and load recording for the harness.
use super::{data, ops::Timing};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Instant,
};

pub fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name}"))
}

pub fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
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

pub fn dataset_tasks(case: &str) -> u64 {
    if case.starts_with("large") { 5000 } else { 300 }
}

/// Task index and query kind for sample `i`: identical across paths.
pub fn draw(case: &str, i: u64) -> (u64, u64) {
    let mut rng = data::Rng::new(data::SEED ^ (i + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    (i % 3, rng.below(dataset_tasks(case)))
}

pub struct Child {
    pub wall_us: u64,
    pub timing: Option<Timing>,
}

pub fn spawn(op: &str, root: &Path, kind: u64, task: u64, extra: &[(&str, String)]) -> Child {
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
        .env("HF_MEASURE_OP", op)
        .env("HF_MEASURE_ROOT", root)
        .env("HF_MEASURE_KIND", kind.to_string())
        .env("HF_MEASURE_TASK", task.to_string())
        .envs(extra.iter().map(|(k, v)| (*k, v.as_str())))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .unwrap();
    let wall_us = super::ops::micros(started);
    let text = String::from_utf8_lossy(&out.stdout);
    let timing = text.lines().find_map(|line| {
        let at = line.find("HFM ")?;
        let mut parts = line[at + 4..].split(' ');
        Some(Timing {
            open_us: parts.next()?.parse().ok()?,
            query_us: parts.next()?.parse().ok()?,
            err: parts.next().unwrap_or("").to_owned(),
        })
    });
    Child { wall_us, timing }
}

pub fn sample_line(file: &mut File, case: &str, path: &str, i: u64, kind: u64, child: &Child) {
    let (open, query, err) = child.timing.as_ref().map_or((0, 0, "ChildFailed"), |t| {
        (
            t.open_us,
            t.query_us,
            if t.err.is_empty() { "" } else { t.err.as_str() },
        )
    });
    writeln!(
        file,
        r#"{{"case":"{case}","path":"{path}","i":{i},"kind":{kind},"wall_us":{},"open_us":{open},"query_us":{query},"err":"{err}"}}"#,
        child.wall_us
    )
    .unwrap();
}
