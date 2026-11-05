#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::super::support::*;
use crate::{Limits, Store};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
pub(super) struct StageActor {
    child: Child,
    _input: Option<ChildStdin>,
    lines: mpsc::Receiver<Option<String>>,
}
impl StageActor {
    pub(super) fn spawn(root: &Path, stage: &str, expire_ms: Option<u64>) -> Self {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "tests::task_watch_tests::atomicity::actors::task_watch_commit_boundary_actor",
                "--nocapture",
            ])
            .env("HF_WATCH_STAGE_ROOT", root)
            .env("HF_WATCH_STAGE", stage)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(expire_ms) = expire_ms {
            command.env("HF_WATCH_STAGE_EXPIRE_MS", expire_ms.to_string());
        }
        let mut child = command.spawn().unwrap();
        let output = child.stdout.take().unwrap();
        let (send, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if let Ok(line) = line
                    && (line.starts_with("STAGE:") || line.starts_with("WATCH_STAGE_RESULT:"))
                {
                    let _ = send.send(Some(line));
                }
            }
            let _ = send.send(None);
        });
        let input = child.stdin.take();
        Self {
            child,
            _input: input,
            lines,
        }
    }
    pub(super) fn wait_at_stage(&self, stage: &str) {
        assert_eq!(
            self.lines
                .recv_timeout(Duration::from_secs(5))
                .expect("stage deadline")
                .as_deref(),
            Some(stage)
        );
    }
    pub(super) fn wait_marker(&self, marker: &str) {
        assert_eq!(
            self.lines
                .recv_timeout(Duration::from_secs(5))
                .expect("actor marker deadline")
                .as_deref(),
            Some(marker)
        );
    }
    pub(super) fn marker_absent_for(&self, duration: Duration) -> bool {
        self.lines.recv_timeout(duration).is_err()
    }
    pub(super) fn release_stage(&mut self) {
        writeln!(self._input.as_mut().unwrap(), "CONTINUE").unwrap();
    }
    pub(super) fn wait_success(&mut self) {
        assert!(
            self.lines
                .recv_timeout(Duration::from_secs(5))
                .expect("actor exit deadline")
                .is_none()
        );
        let status = reap_until(&mut self.child, Instant::now() + Duration::from_secs(3))
            .expect("actor status remains observable")
            .expect("successful actor must be reaped");
        assert!(
            status.success(),
            "actor failed after controlled deadline release: {status}"
        );
    }
    pub(super) fn kill_and_reap(&mut self) {
        let state = self
            .child
            .try_wait()
            .expect("child state query must succeed before signal");
        assert!(
            state.is_none(),
            "boundary actor exited before controlled termination: {state:?}"
        );
        self.child
            .kill()
            .expect("confirmed-live boundary actor must accept termination");
        let status = reap_until(&mut self.child, Instant::now() + Duration::from_secs(3))
            .expect("actor process status must remain observable after kill")
            .expect("boundary actor must be reaped within the cleanup deadline");
        assert!(
            !status.success(),
            "boundary actor should have been killed while blocked at its hook"
        );
    }
}
impl Drop for StageActor {
    fn drop(&mut self) {
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                if let Err(error) = self.child.kill() {
                    eprintln!(
                        "watch stage actor cleanup could not signal confirmed-live child: {error}"
                    );
                    return;
                }
                match reap_until(&mut self.child, Instant::now() + Duration::from_secs(3)) {
                    Ok(Some(_)) => {}
                    Ok(None) => eprintln!(
                        "watch stage actor cleanup could not confirm reaping before deadline"
                    ),
                    Err(error) => eprintln!(
                        "watch stage actor cleanup could not observe child status: {error}"
                    ),
                }
            }
            Err(error) => eprintln!(
                "watch stage actor cleanup could not establish child state; left unsignalled: {error}"
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

#[test]
fn task_watch_commit_boundary_actor() {
    let Ok(root) = std::env::var("HF_WATCH_STAGE_ROOT") else {
        return;
    };
    let _guard = crate::tests::fixture_guard();
    let stage = std::env::var("HF_WATCH_STAGE").unwrap();
    crate::tests::select_stage(stage);
    let store = Store::open(
        root,
        Limits::default(),
        deadline(),
        crate::tests::lease::lease_port(),
    )
    .unwrap();
    if let Ok(milliseconds) = std::env::var("HF_WATCH_STAGE_EXPIRE_MS") {
        let deadline = Instant::now() + Duration::from_millis(milliseconds.parse().unwrap());
        assert!(matches!(
            store.replace_task_fixture(&task_fixture(&[("atomic", b"after")]), deadline),
            Err(crate::Error::DeadlineExceeded)
        ));
        println!("WATCH_STAGE_RESULT:deadline-rollback");
        return;
    }
    publish(&store, &[("atomic", b"after")]);
}
