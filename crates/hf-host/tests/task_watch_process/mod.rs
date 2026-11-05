#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use hf_application::{task_read::TaskRow, task_watch::WatchControl, task_watch::WatchError};
use hf_domain::task_read::{Coverage, StoredObservation, TaskId};
use hf_storage::TaskFixture;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::{Arc, Condvar, Mutex, MutexGuard, mpsc},
    thread,
    time::{Duration, Instant},
};
pub(super) static FIXTURE_GATE: Mutex<()> = Mutex::new(());
pub(super) struct Fixture(PathBuf, MutexGuard<'static, ()>);
impl Fixture {
    pub(super) fn new() -> Self {
        let guard = FIXTURE_GATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-host-watch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap(), guard)
    }
    pub(super) fn root(&self) -> PathBuf {
        self.0.join("store")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
        let _ = &self.1;
    }
}
pub(super) fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(8)
}
pub(super) fn task_fixture() -> TaskFixture {
    TaskFixture {
        rows: vec![TaskRow {
            id: TaskId::new("published-by-child".into()).unwrap(),
            stamp: 1,
            session: None,
            observation: StoredObservation {
                recorded_status: None,
                active: None,
                exited: None,
                drained: None,
                owned: None,
            },
            header: b"separate-process-commit".to_vec(),
            status: None,
        }],
        pending: vec![],
        coverage: Coverage {
            discovery_complete: true,
            ownership_complete: true,
            source_complete: true,
        },
    }
}

pub(super) struct ProcessGuard {
    child: Child,
    input: Option<ChildStdin>,
    lines: mpsc::Receiver<Option<String>>,
}
impl ProcessGuard {
    pub(super) fn spawn(root: &Path) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "task_watch_writer_actor", "--nocapture"])
            .env("HF_WATCH_ACTOR_ROOT", root)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) if line.starts_with("WATCH_ACTOR_") => {
                        let _ = send.send(Some(line));
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            let _ = send.send(None);
        });
        let input = child.stdin.take();
        Self {
            child,
            input,
            lines,
        }
    }
    pub(super) fn wait_for(&self, expected: &str) {
        match self
            .lines
            .recv_timeout(Duration::from_secs(5))
            .expect("writer actor marker deadline")
        {
            Some(line) if line == expected => {}
            Some(other) => panic!("unexpected actor marker {other}"),
            None => panic!("actor output closed before {expected}"),
        }
    }
    pub(super) fn go(&mut self) {
        writeln!(self.input.as_mut().unwrap(), "GO").unwrap();
    }
    pub(super) fn wait_success(&mut self) {
        if let Some(line) = self
            .lines
            .recv_timeout(Duration::from_secs(5))
            .expect("writer actor exit deadline")
        {
            panic!("unexpected actor marker after commit: {line}");
        }
        let status = reap_until(&mut self.child, Instant::now() + Duration::from_secs(3))
            .expect("actor status remains observable")
            .expect("actor must be reaped before cleanup deadline");
        assert!(
            status.success(),
            "writer helper exited unsuccessfully: {status}"
        );
    }
}
impl Drop for ProcessGuard {
    fn drop(&mut self) {
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                if let Err(error) = self.child.kill() {
                    eprintln!(
                        "task watch writer cleanup could not signal confirmed-live child: {error}"
                    );
                    return;
                }
                match reap_until(&mut self.child, Instant::now() + Duration::from_secs(3)) {
                    Ok(Some(_)) => {}
                    Ok(None) => eprintln!(
                        "task watch writer cleanup could not confirm reaping before deadline"
                    ),
                    Err(error) => eprintln!(
                        "task watch writer cleanup could not observe child status: {error}"
                    ),
                }
            }
            Err(error) => eprintln!(
                "task watch writer cleanup could not establish child state; left unsignalled: {error}"
            ),
        }
    }
}
fn reap_until(child: &mut Child, deadline: Instant) -> std::io::Result<Option<ExitStatus>> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::yield_now();
    }
}

#[derive(Default)]
pub(super) struct PollControl(pub(super) Arc<PollState>);
#[derive(Default)]
pub(super) struct PollState {
    wake: Mutex<bool>,
    changed: Condvar,
    pub(super) entered: Mutex<Option<mpsc::Sender<()>>>,
}
impl WatchControl for PollControl {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn check(&self) -> Result<(), WatchError> {
        Ok(())
    }
    fn wait_until(&self, target: Instant) -> Result<(), WatchError> {
        if let Some(send) = self.0.entered.lock().unwrap().take() {
            let _ = send.send(());
        }
        let mut wake = self
            .0
            .wake
            .lock()
            .map_err(|_| WatchError::ControlUnavailable)?;
        loop {
            let remaining = target.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(());
            }
            (wake, _) = self
                .0
                .changed
                .wait_timeout(wake, remaining)
                .map_err(|_| WatchError::ControlUnavailable)?;
        }
    }
}
