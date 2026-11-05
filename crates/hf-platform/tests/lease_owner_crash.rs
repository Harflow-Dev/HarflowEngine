#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_application::lease::LeaseMode;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-lease-crash-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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

/// A fresh process holds the real platform lease until the parent kills it.
#[test]
fn lease_owner_crash_helper() {
    let Ok(path) = std::env::var("HARFLOW_R04C_LEASE_HOLDER_PATH") else {
        return;
    };
    let _lease = hf_platform::lease::shared()
        .acquire(&PathBuf::from(path), LeaseMode::Exclusive, true, deadline())
        .expect("acquire helper lease");
    println!("READY");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
}

#[test]
fn owner_process_crash_releases_only_after_process_exit_and_external_probe_confirms_it() {
    let fixture = Fixture::new();
    let path = fixture.0.join("root.lease");
    let holder = HolderChild::spawn(&path);
    let provider = hf_platform::lease::shared();
    assert_eq!(
        provider
            .acquire(&path, LeaseMode::Exclusive, true, deadline())
            .err(),
        Some(hf_application::lease::LeaseError::Busy)
    );
    assert_eq!(external_fcntl_probe(&path), "BLOCKED");

    holder.kill_and_reap();
    let acquired = provider
        .acquire(&path, LeaseMode::Exclusive, true, deadline())
        .expect("lease becomes available after owner process exits");
    assert_eq!(external_fcntl_probe(&path), "BLOCKED");
    drop(acquired);
    assert_eq!(external_fcntl_probe(&path), "ACQUIRED");
}

struct HolderChild {
    child: Child,
    _input: ChildStdin,
    reader: Option<std::thread::JoinHandle<()>>,
}
impl HolderChild {
    fn spawn(path: &Path) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "lease_owner_crash_helper", "--nocapture"])
            .env("HARFLOW_R04C_LEASE_HOLDER_PATH", path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().expect("holder stdin");
        let stdout = child.stdout.take().expect("holder stdout");
        let (send, receive) = mpsc::sync_channel::<String>(1);
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let ready = line.trim() == "READY";
                if send.send(line).is_err() || ready {
                    return;
                }
            }
        });
        let mut holder = Self {
            child,
            _input: input,
            reader: Some(reader),
        };
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            let remaining = end.saturating_duration_since(Instant::now());
            let line = receive
                .recv_timeout(remaining)
                .expect("lease holder readiness deadline");
            if line.trim() == "READY" {
                break;
            }
        }
        join_reader(holder.reader.take().expect("holder reader"));
        if holder.child.try_wait().unwrap().is_some() {
            panic!("lease holder exited before parent probe");
        }
        holder
    }

    fn kill_and_reap(mut self) {
        self.child.kill().expect("kill lease holder process");
        reap_or_abort(&mut self.child);
    }
}
impl Drop for HolderChild {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            reap_or_abort(&mut self.child);
        }
        if let Some(reader) = self.reader.take() {
            join_reader(reader);
        }
    }
}

struct ProbeChild(Child);
impl Drop for ProbeChild {
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

fn wait_child(child: &mut Child) -> Result<ExitStatus, String> {
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < end => std::thread::yield_now(),
            Ok(None) => {
                let _ = child.kill();
                reap_or_abort(child);
                return Err("external fcntl probe timed out".into());
            }
            Err(error) => {
                let _ = child.kill();
                reap_or_abort(child);
                return Err(error.to_string());
            }
        }
    }
}

fn external_fcntl_probe(path: &Path) -> String {
    let script = "import fcntl,os,sys; f=os.open(sys.argv[1],os.O_RDWR);\ntry:\n fcntl.lockf(f,fcntl.LOCK_EX|fcntl.LOCK_NB); print('ACQUIRED')\nexcept BlockingIOError: print('BLOCKED')";
    let mut child = ProbeChild(
        Command::new("/usr/bin/python3")
            .args(["-I", "-S", "-c", script])
            .arg(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (send, receive) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(65).read_to_end(&mut bytes).map(|_| bytes);
        let _ = send.send(result);
    });
    let status = wait_child(&mut child.0);
    let output = receive
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| "external fcntl output timeout".to_owned())
        .and_then(|result| result.map_err(|error| error.to_string()));
    join_reader(reader);
    let status = status.expect("external fcntl probe wait failed");
    assert!(status.success(), "external fcntl probe failed: {status:?}");
    let output = output.expect("external fcntl output read failed");
    assert!(output.len() <= 64, "external fcntl output exceeded bound");
    String::from_utf8(output).unwrap().trim().to_owned()
}
