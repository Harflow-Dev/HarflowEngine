//! Child-process actor harness. The parent waits only on bounded reads of the
//! child's stdout; the child reports its own outcome so the parent stays an
//! independent observer of the ledger.
use super::fixture::{Fixture, cancel_intent, claim_intent, deadline, id, limits, send_intent};
use crate::{Error, Store};
use hf_application::conversation::Command;
use hf_domain::conversation::{ClaimIntent, Ledger};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command as Process, Stdio},
    sync::mpsc::{Receiver, RecvTimeoutError, channel},
    time::{Duration, Instant},
};

const READ_BOUND: Duration = Duration::from_secs(20);
const TEST: &str = "tests::conversation_tests::actor::conversation_actor_child";

/// Request ids a child derives from its index, shared with the parent so it can
/// rebuild the exact intent for a replay.
pub(super) fn child_send(ledger: &Ledger, n: u32) -> hf_domain::conversation::SendIntent {
    send_intent(ledger, 130 + n, "actor")
}
pub(super) fn child_claim(ledger: &Ledger, n: u32) -> ClaimIntent {
    claim_intent(ledger, 150 + n, 170 + n)
}

pub(super) struct Spec {
    op: &'static str,
    n: u32,
    hook: Option<&'static str>,
    target: u32,
    deadline_ms: u64,
    retry: bool,
}
impl Spec {
    pub fn new(op: &'static str, n: u32) -> Self {
        Self {
            op,
            n,
            hook: None,
            target: 30,
            deadline_ms: 8000,
            retry: false,
        }
    }
    pub fn hook(mut self, hook: &'static str) -> Self {
        self.hook = Some(hook);
        self
    }
    pub fn target(mut self, target: u32) -> Self {
        self.target = target;
        self
    }
    pub fn deadline_ms(mut self, ms: u64) -> Self {
        self.deadline_ms = ms;
        self
    }
    pub fn retry_busy(mut self) -> Self {
        self.retry = true;
        self
    }
}

pub(super) struct Actor {
    child: Child,
    lines: Receiver<Option<String>>,
    deadline_ms: u64,
    staged_at: Option<Instant>,
}
impl Drop for Actor {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
impl Actor {
    pub fn spawn(fixture: &Fixture, spec: &Spec) -> Self {
        let mut command = Process::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
            .env("HF_R06_ROOT", fixture.root())
            .env("HF_R06_OP", spec.op)
            .env("HF_R06_N", spec.n.to_string())
            .env("HF_R06_TARGET", spec.target.to_string())
            .env("HF_R06_DEADLINE_MS", spec.deadline_ms.to_string())
            .env("HF_R06_RETRY", u8::from(spec.retry).to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(hook) = spec.hook {
            command.env("HF_R06_HOOK", hook);
        }
        let mut child = command.spawn().unwrap();
        let output = child.stdout.take().unwrap();
        let (sender, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if sender.send(Some(line)).is_err() {
                    return;
                }
            }
            let _ = sender.send(None);
        });
        Self {
            child,
            lines,
            deadline_ms: spec.deadline_ms,
            staged_at: None,
        }
    }
    /// Returns the text from `token` onward. libtest may prefix the line with
    /// "test name ... ", so the token is located rather than anchored.
    fn wait_for(&self, token: &str) -> String {
        loop {
            match self.lines.recv_timeout(READ_BOUND) {
                Ok(Some(line)) => {
                    if let Some(at) = line.find(token) {
                        return line[at..].trim().to_owned();
                    }
                }
                Ok(None) => panic!("actor exited before {token}"),
                Err(RecvTimeoutError::Timeout) => panic!("timed out waiting for {token}"),
                Err(RecvTimeoutError::Disconnected) => panic!("actor reader ended before {token}"),
            }
        }
    }
    fn send_line(&mut self, text: &[u8]) {
        let stdin = self.child.stdin.as_mut().unwrap();
        stdin.write_all(text).unwrap();
        stdin.flush().unwrap();
    }
    pub fn ready(&self) {
        self.wait_for("HF_ACTOR_READY");
    }
    pub fn go(&mut self) {
        self.send_line(b"GO\n");
    }
    pub fn stage(&mut self, hook: &str) {
        self.wait_for(&format!("STAGE:{hook}"));
        self.staged_at = Some(Instant::now());
    }
    /// Lets a child paused at its hook continue.
    pub fn release(&mut self) {
        self.send_line(b"\n");
    }
    /// The child's own report, for example `Ok:grant` or `Err:Busy`.
    pub fn outcome(&self) -> String {
        let line = self.wait_for("HF_OUTCOME:");
        line["HF_OUTCOME:".len()..].to_owned()
    }
    /// Waits for the child's stdout to close, which means the process exited.
    pub fn exited(&mut self) {
        loop {
            match self.lines.recv_timeout(READ_BOUND) {
                Ok(Some(_)) => {}
                Ok(None) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => panic!("actor did not exit"),
            }
        }
        assert!(self.child.wait().unwrap().success());
    }
    pub fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
    /// The only timed wait in these tests. The child starts its deadline after
    /// reading GO, which precedes the hook line the parent observes, so waiting
    /// one full deadline from that observation always passes the child's own.
    pub fn wait_past_own_deadline(&self) {
        let until = self.staged_at.unwrap() + Duration::from_millis(self.deadline_ms);
        std::thread::sleep(until.saturating_duration_since(Instant::now()));
    }
}

fn describe<T>(result: Result<T, Error>, ok: impl FnOnce(T) -> &'static str) -> String {
    match result {
        Ok(value) => ok(value).to_owned(),
        Err(error) => format!("Err:{error:?}"),
    }
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap()
}

fn run(store: &Store, ledger: &Ledger, op: &str, n: u32, end: std::time::Instant) -> String {
    let conversation = id(7);
    let target: u32 = env("HF_R06_TARGET").parse().unwrap();
    match op {
        "send" => describe(
            store.conversation_apply(&conversation, Command::Send(child_send(ledger, n)), end),
            |_| "Ok",
        ),
        "claim" => describe(
            store.conversation_apply(&conversation, Command::Claim(child_claim(ledger, n)), end),
            |o| {
                if o.grant.is_some() {
                    "Ok:grant"
                } else {
                    "Ok:nogrant"
                }
            },
        ),
        "cancel" => {
            let intent = cancel_intent(ledger, 190 + n, target);
            describe(
                store.conversation_apply(&conversation, Command::Cancel(intent), end),
                |_| "Ok",
            )
        }
        "observe" => describe(store.conversation_observe(&conversation, end), |_| "Ok"),
        other => panic!("unknown operation {other}"),
    }
}

#[test]
fn conversation_actor_child() {
    let Ok(root) = std::env::var("HF_R06_ROOT") else {
        return;
    };
    let store = Store::open(
        root,
        limits(),
        deadline(),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    let ledger = store.conversation_observe(&id(7), deadline()).unwrap();
    let (op, n) = (env("HF_R06_OP"), env("HF_R06_N").parse::<u32>().unwrap());
    if let Ok(hook) = std::env::var("HF_R06_HOOK") {
        crate::tests::select_stage(hook);
    }
    println!("HF_ACTOR_READY");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    let end = Instant::now() + Duration::from_millis(env("HF_R06_DEADLINE_MS").parse().unwrap());
    let retry = env("HF_R06_RETRY") == "1";
    let outcome = loop {
        let outcome = run(&store, &ledger, &op, n, end);
        if retry && outcome == "Err:Busy" && Instant::now() < end {
            std::thread::yield_now();
            continue;
        }
        break outcome;
    };
    println!("HF_OUTCOME:{outcome}");
    let _ = std::io::stdout().flush();
}
