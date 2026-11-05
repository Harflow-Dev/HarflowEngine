use super::{Fixture, deadline};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::Instant,
};

mod execute;
pub(in crate::tests::capture_atomicity_tests) use execute::actor_child;

struct ChildGuard(Child);
impl ChildGuard {
    fn cleanup(&mut self) -> Result<(), String> {
        match self.0.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => self
                .0
                .kill()
                .map_err(|e| format!("actor kill failed: {e}"))?,
            Err(e) => return Err(format!("actor status unknown; refusing kill: {e}")),
        }
        let until = deadline();
        loop {
            match self.0.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) if Instant::now() < until => thread::yield_now(),
                Ok(None) => return Err("actor was not reaped by deadline".into()),
                Err(e) => return Err(format!("actor status unknown during reap: {e}")),
            }
        }
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            if thread::panicking() {
                eprintln!("capture actor cleanup failed during unwind: {error}");
            } else {
                panic!("capture actor cleanup failed: {error}");
            }
        }
    }
}

pub(in crate::tests::capture_atomicity_tests) struct Actor {
    child: ChildGuard,
    input: Option<ChildStdin>,
    output: mpsc::Receiver<Option<String>>,
}
impl Actor {
    pub(in crate::tests::capture_atomicity_tests) fn spawn(
        fixture: &Fixture,
        action: &str,
        hook: &str,
    ) -> Self {
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::capture_atomicity_tests::capture_actor_child",
                "--nocapture",
            ])
            .env("HARFLOW_B2C_ROOT", fixture.root())
            .env("HARFLOW_B2C_ACTION", action)
            .env("HARFLOW_B2C_HOOK", hook)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut child = ChildGuard(child);
        let stdout = child.0.stdout.take().unwrap();
        let input = child.0.stdin.take();
        let (send, output) = mpsc::channel();
        thread::spawn(move || read_lines(stdout, send));
        let mut actor = Self {
            child,
            input,
            output,
        };
        assert_eq!(actor.line("ACTOR_READY"), "ACTOR_READY");
        actor
    }
    pub(in crate::tests::capture_atomicity_tests) fn line(&mut self, prefix: &str) -> String {
        let until = deadline();
        loop {
            let left = until.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "actor timed out waiting for {prefix}");
            match self.output.recv_timeout(left) {
                Ok(Some(line)) if line.starts_with(prefix) => return line,
                Ok(Some(_)) => continue,
                Ok(None) => panic!("actor exited before {prefix}"),
                Err(_) => panic!("actor timed out waiting for {prefix}"),
            }
        }
    }
    pub(in crate::tests::capture_atomicity_tests) fn go(&mut self) {
        let input = self.input.as_mut().expect("actor stdin available");
        input.write_all(b"GO\n").unwrap();
        input.flush().unwrap();
    }
    pub(in crate::tests::capture_atomicity_tests) fn kill_and_reap(&mut self) {
        match self.child.0.try_wait() {
            Ok(Some(_)) => panic!("actor exited before intentional cut"),
            Err(error) => panic!("actor status unknown; refusing kill: {error}"),
            Ok(None) => self.child.0.kill().unwrap(),
        }
        self.child.cleanup().unwrap();
        self.reader_eof().unwrap();
    }
    fn reader_eof(&mut self) -> Result<(), String> {
        let until = deadline();
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("actor output reader did not finish by deadline".into());
            }
            match self.output.recv_timeout(left) {
                Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                Ok(Some(_)) => continue,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err("actor output reader did not finish by deadline".into());
                }
            }
        }
    }
}
impl Drop for Actor {
    fn drop(&mut self) {
        let result = self.child.cleanup();
        let result = result.and_then(|()| self.reader_eof());
        if let Err(error) = result {
            if thread::panicking() {
                eprintln!("capture actor cleanup failed during unwind: {error}");
            } else {
                panic!("capture actor cleanup failed: {error}");
            }
        }
    }
}

fn read_lines(stdout: impl Read, send: mpsc::Sender<Option<String>>) {
    for line in BufReader::new(stdout).lines() {
        match line {
            Ok(line) => {
                let _ = send.send(Some(line));
            }
            Err(_) => break,
        }
    }
    let _ = send.send(None);
}
