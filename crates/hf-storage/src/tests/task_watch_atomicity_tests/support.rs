use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const LIMIT: Duration = Duration::from_secs(5);

pub(super) struct Fixture {
    pub(super) base: PathBuf,
    _guard: std::sync::MutexGuard<'static, ()>,
}
impl Fixture {
    pub(super) fn new() -> Self {
        let guard = crate::tests::fixture_guard();
        let base = std::env::temp_dir().join(format!("hf-watch-atomic-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir(&base).unwrap();
        Self {
            base: fs::canonicalize(base).unwrap(),
            _guard: guard,
        }
    }
    pub(super) fn root(&self) -> PathBuf {
        self.base.join("store")
    }
    pub(super) fn raw(&self) -> rusqlite::Connection {
        rusqlite::Connection::open_with_flags(
            self.root().join("store.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    }
    pub(super) fn ledger(&self) -> PathBuf {
        self.base.join("committed-ledger")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

pub(super) struct Actor {
    child: Child,
    input: ChildStdin,
    lines: mpsc::Receiver<String>,
}
impl Actor {
    pub(super) fn spawn(root: &Path, stage: Option<&str>, ledger: &Path) -> Self {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "tests::task_watch_atomicity_tests::actor_child",
                "--nocapture",
            ])
            .env("HF_WATCH_ATOMIC_ROOT", root)
            .env("HF_WATCH_ATOMIC_LEDGER", ledger)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(stage) = stage {
            command.env("HF_WATCH_ATOMIC_STAGE", stage);
        }
        let mut child = command.spawn().unwrap();
        let output = child.stdout.take().unwrap();
        let (send, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(output).lines().take(32) {
                match line {
                    Ok(line)
                        if line.starts_with("READY")
                            || line.starts_with("STAGE:")
                            || line.starts_with("RESULT:") =>
                    {
                        let _ = send.send(line);
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });
        let actor = Self {
            input: child.stdin.take().unwrap(),
            child,
            lines,
        };
        actor.expect("READY");
        actor
    }
    pub(super) fn send(&mut self, line: &str) {
        writeln!(self.input, "{line}").unwrap();
        self.input.flush().unwrap();
    }
    pub(super) fn expect(&self, expected: &str) {
        assert_eq!(
            self.lines
                .recv_timeout(LIMIT)
                .expect("bounded actor output"),
            expected
        );
    }
    pub(super) fn terminate(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            if self.child.kill().is_err() {
                assert!(
                    self.child.try_wait().unwrap().is_some(),
                    "owned child kill failed while still running"
                );
                return;
            }
            let end = Instant::now() + LIMIT;
            loop {
                if self.child.try_wait().unwrap().is_some() {
                    return;
                }
                assert!(
                    Instant::now() < end,
                    "owned actor did not reap within deadline"
                );
                thread::yield_now();
            }
        }
    }
}
impl Drop for Actor {
    fn drop(&mut self) {
        self.terminate();
    }
}
