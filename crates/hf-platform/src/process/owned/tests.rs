use super::*;
use crate::process::MacIdentity;
use std::{
    ffi::OsString,
    io::Read,
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

struct ExternalReapGuard(Pid, bool);
impl Drop for ExternalReapGuard {
    fn drop(&mut self) {
        if self.1 {
            return;
        }
        let end = Instant::now() + Duration::from_secs(2);
        loop {
            match waitpid(self.0, Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::Exited(_, _) | WaitStatus::Signaled(_, _, _)) => return,
                Ok(_) | Err(nix::errno::Errno::EINTR) if Instant::now() < end => {
                    std::thread::yield_now();
                }
                _ => std::process::abort(),
            }
        }
    }
}

struct ProbeChild(Child);
impl Drop for ProbeChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            reap_probe_or_abort(&mut self.0);
        }
    }
}

fn reap_probe_or_abort(child: &mut Child) {
    let end = Instant::now() + Duration::from_secs(1);
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

fn wait_probe(child: &mut Child) -> Result<ExitStatus, String> {
    let end = Instant::now() + Duration::from_secs(1);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < end => std::thread::yield_now(),
            Ok(None) => {
                let _ = child.kill();
                reap_probe_or_abort(child);
                return Err("ps witness timed out".into());
            }
            Err(error) => {
                let _ = child.kill();
                reap_probe_or_abort(child);
                return Err(error.to_string());
            }
        }
    }
}

fn join_probe_reader(reader: std::thread::JoinHandle<()>) {
    let end = Instant::now() + Duration::from_secs(1);
    while !reader.is_finished() && Instant::now() < end {
        std::thread::yield_now();
    }
    if !reader.is_finished() {
        std::process::abort();
    }
    let _ = reader.join();
}

fn process_state(pid: u32) -> String {
    let mut child = ProbeChild(
        Command::new("/bin/ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (send, receive) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(257).read_to_end(&mut bytes).map(|_| bytes);
        let _ = send.send(result);
    });
    let status = wait_probe(&mut child.0);
    let output = receive
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| "ps output timed out".to_owned())
        .and_then(|result| result.map_err(|error| error.to_string()));
    join_probe_reader(reader);
    assert!(status.unwrap().success(), "ps witness failed");
    let output = output.expect("ps output read failed");
    assert!(output.len() <= 256, "ps output exceeded its bound");
    String::from_utf8(output).unwrap().trim().to_owned()
}

fn zombie_witness(pid: u32, deadline: Instant) {
    loop {
        if process_state(pid).starts_with('Z') {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "independent zombie witness expired"
        );
        std::thread::yield_now();
    }
}

#[test]
fn modeled_lost_wait_authority_preserves_unobserved_child_for_external_reaper() {
    let cwd = std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
    let mut child = RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: PathBuf::from("/bin/cat"),
            cwd,
            args: Vec::<OsString>::new(),
            environment: Vec::new(),
            capture_limit: 128,
        },
        &MacIdentity,
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    let pid = child.child;
    let mut reaper = ExternalReapGuard(pid, false);
    let expected = child.registered_identity().cloned();
    let live_identity = expected
        .as_ref()
        .and_then(|identity| MacIdentity.identity(identity.pid()));
    child.close_stdin();
    let expected = expected.expect("spawned child identity");
    assert_eq!(expected.pid(), pid.as_raw() as u32);
    assert_eq!(live_identity.as_ref(), Some(&expected));
    // RecoveryIdentityPort conservatively returns Unknown for this child
    // zombie, so the independent ps state is the positive nonreap witness.
    zombie_witness(expected.pid(), Instant::now() + Duration::from_secs(2));

    // This models a previous ECHILD observation or competing waiter, without
    // claiming an actual PID reuse or external reap before the witness above.
    child.wait_authority_lost = true;
    child.authority_unknown = true;
    assert_eq!(child.try_wait().unwrap(), ProcessExit::Unobserved);
    assert_eq!(
        child
            .request_signal(
                &expected,
                &MacIdentity,
                OwnedSignal::Kill,
                Instant::now() + Duration::from_secs(1),
            )
            .unwrap(),
        SignalOutcome::Refused(IdentityObservation::Unknown)
    );

    assert!(matches!(
        waitpid(pid, Some(WaitPidFlag::WNOHANG)).unwrap(),
        WaitStatus::Exited(_, 0)
    ));
    reaper.1 = true;
}
