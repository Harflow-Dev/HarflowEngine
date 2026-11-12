#![cfg(unix)]

use super::{Limits, Store};
use hf_domain::ownership::{Digest, Id, OwnerClaim, PreparedRequest};
use hf_domain::process::ProcessIdentity;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use sha2::{Digest as _, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

pub(super) const WAIT: Duration = Duration::from_secs(8);
pub(super) fn deadline() -> Instant {
    Instant::now() + WAIT
}

pub(super) struct Fixture(
    PathBuf,
    #[allow(dead_code)] std::sync::MutexGuard<'static, ()>,
);
impl Fixture {
    pub(super) fn new(label: &str) -> Self {
        let guard = super::super::fixture_guard();
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-own-atomic-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap(), guard)
    }
    pub(super) fn root(&self) -> PathBuf {
        self.0.join("store")
    }
    pub(super) fn ledger(&self) -> PathBuf {
        self.0.join("granted.log")
    }
    pub(super) fn store(&self) -> Store {
        Store::initialize_fixture(
            self.root(),
            Limits::default(),
            deadline(),
            crate::tests::lease::lease_port(),
        )
        .unwrap()
    }
    pub(super) fn open_store(&self) -> Store {
        Store::open(
            self.root(),
            Limits::default(),
            deadline(),
            crate::tests::lease::lease_port(),
        )
        .unwrap()
    }
    pub(super) fn raw_readonly(&self) -> Connection {
        Connection::open_with_flags(
            self.root().join("store.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    }
    pub(super) fn raw_writer(&self) -> Connection {
        crate::tests::raw_connection(self.root().join("store.sqlite3"))
    }
    pub(super) fn raw_child(&self, operation: &Id) -> Option<String> {
        self.raw_readonly()
            .query_row(
                "SELECT child FROM ownership_records WHERE operation=?1",
                [operation.as_str()],
                |row| row.get(0),
            )
            .optional()
            .unwrap()
    }
    pub(super) fn ledger_lines(&self) -> Vec<String> {
        fs::read_to_string(self.ledger())
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn id(value: u8) -> Id {
    Id::new(format!("{value:032x}")).unwrap()
}
pub(super) fn tag(value: &Id) -> u8 {
    u8::from_str_radix(&value.as_str()[30..], 16).unwrap()
}
pub(super) fn request(
    store: &Store,
    op: u8,
    session: u8,
    reservation: u8,
    attempt: u8,
) -> PreparedRequest {
    let payload = format!("ownership atomicity fixture {op}").into_bytes();
    PreparedRequest {
        operation: id(op),
        session: id(session),
        reservation: id(reservation),
        attempt: id(attempt),
        generation: Id::new(store.identity().generation.clone()).unwrap(),
        digest: Digest::new(format!("{:x}", Sha256::digest(&payload))).unwrap(),
        payload,
    }
}
pub(super) fn owner(tag: u8) -> OwnerClaim {
    OwnerClaim {
        owner: id(tag),
        nonce: id(tag + 1),
        process: process(100 + u32::from(tag)),
    }
}
pub(super) fn append_grant(ledger: &Path, operation: &Id, owner: &Id) {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ledger)
        .unwrap();
    writeln!(file, "{}|{}", operation.as_str(), owner.as_str()).unwrap();
    file.sync_all().unwrap();
}
pub(super) fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 12, 34, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}
pub(super) fn raw_state(
    connection: &Connection,
    operation: &Id,
) -> Option<(i64, i64, bool, Option<String>)> {
    connection.query_row(
        "SELECT phase,revision,cancelled,release_proof FROM ownership_records WHERE operation=?1",
        [operation.as_str()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).optional().unwrap()
}

pub(super) struct ActorGuard {
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Receiver<Option<String>>,
}
impl ActorGuard {
    pub(super) fn spawn(
        root: &Path,
        ledger: &Path,
        action: &str,
        op: u8,
        args: &[(&str, String)],
    ) -> Self {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "tests::ownership_atomicity_tests::ownership_actor_child",
                "--nocapture",
            ])
            .env("HF_B1_ACTOR", "1")
            .env("HF_B1_ROOT", root)
            .env("HF_B1_LEDGER", ledger)
            .env("HF_B1_ACTION", action)
            .env("HF_B1_OP", op.to_string())
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .stderr(Stdio::inherit());
        for (key, value) in args {
            command.env(key, value);
        }
        let mut child = command.spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, messages) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if line.starts_with("ACTOR_") || line.starts_with("STAGE:") {
                    let _ = send.send(Some(line));
                }
            }
            let _ = send.send(None);
        });
        let actor = Self {
            stdin: child.stdin.take(),
            child,
            messages,
        };
        actor.wait_for("ACTOR_READY", deadline()).unwrap();
        actor
    }
    pub(super) fn wait_for(&self, prefix: &str, until: Instant) -> Result<String, String> {
        loop {
            let remaining = until.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!("timed out waiting for {prefix}"));
            }
            match self.messages.recv_timeout(remaining) {
                Ok(Some(line)) if line.starts_with(prefix) => return Ok(line),
                Ok(Some(_)) => continue,
                Ok(None) => return Err(format!("actor exited before {prefix}")),
                Err(_) => return Err(format!("timed out waiting for {prefix}")),
            }
        }
    }
    pub(super) fn go(&mut self) -> std::io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| std::io::Error::other("actor stdin closed"))?;
        stdin.write_all(b"GO\n")?;
        stdin.flush()
    }
    pub(super) fn take_input(&mut self) -> ChildStdin {
        self.stdin
            .take()
            .expect("actor stdin was already transferred")
    }
    pub(super) fn kill_and_reap(&mut self) -> Result<(), String> {
        self.cleanup()
    }
    pub(super) fn finish(&mut self, until: Instant) -> Result<(), String> {
        self.reap(until)?;
        self.reader_eof(until)
    }
    fn reader_eof(&mut self, until: Instant) -> Result<(), String> {
        loop {
            let remaining = until.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("actor output reader did not reach EOF".into());
            }
            match self.messages.recv_timeout(remaining) {
                Ok(None) => return Ok(()),
                Ok(Some(_)) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err("actor output reader did not reach EOF".into());
                }
            }
        }
    }
    fn reap(&mut self, until: Instant) -> Result<(), String> {
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) if Instant::now() < until => thread::yield_now(),
                Ok(None) => return Err("actor did not exit before deadline".into()),
                Err(error) => return Err(format!("actor status unknown: {error}")),
            }
        }
    }
    fn cleanup(&mut self) -> Result<(), String> {
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => self
                .child
                .kill()
                .map_err(|e| format!("actor kill failed: {e}"))?,
            Err(error) => return Err(format!("actor status unknown; refusing kill: {error}")),
        }
        let until = deadline();
        self.reap(until)?;
        self.reader_eof(until)
    }
}
impl Drop for ActorGuard {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup()
            && !thread::panicking()
        {
            panic!("actor cleanup uncertain: {error}");
        }
    }
}

pub(super) fn args_for_request(req: &PreparedRequest) -> Vec<(&'static str, String)> {
    vec![
        ("HF_B1_SESSION", tag(&req.session).to_string()),
        ("HF_B1_RESERVATION", tag(&req.reservation).to_string()),
        ("HF_B1_ATTEMPT", tag(&req.attempt).to_string()),
    ]
}
