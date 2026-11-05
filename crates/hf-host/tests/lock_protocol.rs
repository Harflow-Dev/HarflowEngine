#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_storage::{Error, Limits, Store, probe};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-lock-protocol-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(3)
}

/// A separate process performs the new-reader check with the real platform lease.
#[test]
fn r04c_new_probe_helper() {
    let Ok(root) = std::env::var("HARFLOW_R04C_NEW_PROBE_ROOT") else {
        return;
    };
    let result = match probe(root, hf_platform::lease::shared()) {
        Ok(Some(_)) => "OK",
        Ok(None) => "MISSING",
        Err(Error::FutureSchema) => "FUTURE_SCHEMA",
        Err(Error::Incompatible) => "INCOMPATIBLE",
        Err(_) => "OTHER",
    };
    println!("R04C_RESULT={result}");
}

#[test]
#[ignore = "requires the isolated old executable at HARFLOW_R04C_OLD_PROBE"]
fn actual_old_and_new_executables_refuse_each_others_schema_without_changes() {
    let old = PathBuf::from(
        std::env::var_os("HARFLOW_R04C_OLD_PROBE")
            .expect("set HARFLOW_R04C_OLD_PROBE to the isolated old executable"),
    );
    assert!(old.is_file(), "old helper is not a file: {}", old.display());
    let fixture = Fixture::new();

    let old_root = fixture.0.join("old-schema");
    assert_eq!(run_old(&old, "init", &old_root), "OK");
    let old_before = snapshot_tree(&old_root);
    assert_eq!(run_new_probe(&old_root), "INCOMPATIBLE");
    assert_eq!(snapshot_tree(&old_root), old_before);
    assert_eq!(run_old(&old, "probe", &old_root), "OK");
    assert_eq!(snapshot_tree(&old_root), old_before);

    let new_root = fixture.0.join("new-schema");
    drop(
        Store::initialize_fixture(
            &new_root,
            Limits::default(),
            deadline(),
            hf_platform::lease::shared(),
        )
        .unwrap(),
    );
    let new_before = snapshot_tree(&new_root);
    assert_eq!(run_old(&old, "probe", &new_root), "FUTURE_SCHEMA");
    assert_eq!(snapshot_tree(&new_root), new_before);
    assert_eq!(run_new_probe(&new_root), "OK");
    assert_eq!(snapshot_tree(&new_root), new_before);
}

fn run_old(executable: &Path, action: &str, root: &Path) -> String {
    let mut command = Command::new(executable);
    command.arg(action).arg(root);
    result_line(&String::from_utf8(run_captured(command).unwrap()).unwrap())
}

fn run_new_probe(root: &Path) -> String {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "r04c_new_probe_helper", "--nocapture"])
        .env("HARFLOW_R04C_NEW_PROBE_ROOT", root);
    result_line(&String::from_utf8(run_captured(command).unwrap()).unwrap())
}

struct HelperChild(Child);
impl Drop for HelperChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            reap_or_abort(&mut self.0);
        }
    }
}

fn reap_or_abort(child: &mut Child) {
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        if Instant::now() >= end {
            std::process::abort();
        }
        std::thread::yield_now();
    }
}

fn wait_child(child: &mut Child, duration: Duration) -> Result<ExitStatus, String> {
    let end = Instant::now() + duration;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < end => std::thread::yield_now(),
            Ok(None) => {
                let _ = child.kill();
                reap_or_abort(child);
                return Err("qualification helper wait timed out".into());
            }
            Err(error) => {
                let _ = child.kill();
                reap_or_abort(child);
                return Err(error.to_string());
            }
        }
    }
}

fn join_reader(reader: std::thread::JoinHandle<()>) {
    let end = Instant::now() + Duration::from_secs(2);
    while !reader.is_finished() && Instant::now() < end {
        std::thread::yield_now();
    }
    if !reader.is_finished() {
        std::process::abort();
    }
    let _ = reader.join();
}

fn run_captured(mut command: Command) -> Result<Vec<u8>, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = HelperChild(command.spawn().map_err(|error| error.to_string())?);
    let stdout = child.0.stdout.take().ok_or("helper stdout absent")?;
    let (send, receive) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(4097).read_to_end(&mut bytes).map(|_| bytes);
        let _ = send.send(result);
    });
    let status = wait_child(&mut child.0, Duration::from_secs(5));
    let output = receive
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| "qualification helper output timed out".to_owned())
        .and_then(|result| result.map_err(|error| error.to_string()));
    join_reader(reader);
    let status = status?;
    if !status.success() {
        return Err(format!("qualification helper failed: {status:?}"));
    }
    let bytes = output?;
    if bytes.len() > 4096 {
        return Err("qualification helper output exceeded 4096 bytes".into());
    }
    Ok(bytes)
}

fn result_line(output: &str) -> String {
    output
        .lines()
        .find_map(|line| line.strip_prefix("R04C_RESULT="))
        .expect("qualification helper omitted result line")
        .to_owned()
}

fn snapshot_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries = Vec::new();
    snapshot_into(root, &mut entries);
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
}

fn snapshot_into(root: &Path, entries: &mut Vec<(PathBuf, Vec<u8>)>) {
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        let metadata = fs::symlink_metadata(&path).unwrap();
        let bytes = if metadata.is_dir() {
            Vec::new()
        } else {
            fs::read(&path).unwrap()
        };
        entries.push((path.clone(), bytes));
        if metadata.is_dir() {
            snapshot_into(&path, entries);
        }
    }
}
