#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The shipped `harflow-ctl run` against the scripted agent: a fast child loses nothing, the
//! state root a run creates is private, and every process a run starts is accounted for.
#[path = "run_real_process/mod.rs"]
mod support;
use hf_application::process::IdentityPort;
use hf_domain::process::IdentityObservation as Seen;
use hf_domain::process::ProcessIdentity;
use hf_platform::{
    process::{MacIdentity, observed_executable_path},
    task_runtime::caller::observe_caller,
};
use hf_storage::{Limits, Store};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};
use support::{
    Guard, Next, Report, Scratch, after_wait, alive, cleanup, ctl, run_args, signal_confirmed,
    unsettled,
};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(10)
}
fn wait_for<T>(what: &str, within: Duration, mut probe: impl FnMut() -> Option<T>) -> T {
    let end = Instant::now() + within;
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(25));
    }
}
/// The owner and the agent, from the root's retained-process inventory while they are live.
fn recorded_processes(root: &Path) -> Vec<ProcessIdentity> {
    let store = Store::open_with(
        root,
        Limits::default(),
        deadline(),
        hf_platform::lease::shared(),
        hf_host::task_runtime::store_options::suppressed(),
    )
    .unwrap();
    store.retained_runtime_processes(8, deadline()).unwrap()
}
/// The writer: a live child of the owner whose executable is the ctl binary and that is not
/// the agent. Parent identity includes the owner's start time.
fn writer_of(owner: &ProcessIdentity, agent: &ProcessIdentity) -> ProcessIdentity {
    let ctl = fs::canonicalize(env!("CARGO_BIN_EXE_harflow-ctl")).unwrap();
    wait_for("the writer process", Duration::from_secs(20), || {
        let listing = std::process::Command::new("/bin/ps")
            .args(["-ax", "-o", "pid=,ppid="])
            .output()
            .unwrap();
        let text = String::from_utf8(listing.stdout).unwrap();
        text.lines().find_map(|line| {
            let mut fields = line
                .split_whitespace()
                .map(|f| f.parse::<u32>().unwrap_or(0));
            let (pid, parent) = (fields.next()?, fields.next()?);
            (parent == owner.pid() && pid != agent.pid()).then_some(())?;
            let chain = observe_caller(pid, deadline()).ok()?.chain;
            let (me, up) = (chain.first()?, chain.get(1)?);
            let exe = observed_executable_path(&me.identity, deadline()).ok()?;
            (up.identity == *owner && fs::canonicalize(exe).ok()? == ctl)
                .then(|| me.identity.clone())
        })
    })
}
fn assert_private_and_lossless(scratch: &Scratch) {
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode(&scratch.root),
        0o700,
        "the root a run creates is owner-only"
    );
    let payload = scratch.payload();
    assert_eq!(
        fs::read(payload.join("loss.raw")).unwrap(),
        b"",
        "no known loss"
    );
    let (out, err) = (scratch.out(), scratch.err());
    assert!(!out.is_empty());
    assert_eq!(fs::read(payload.join("stdout.raw")).unwrap(), out);
    assert_eq!(fs::read(payload.join("stderr.raw")).unwrap(), err);
    assert!(!payload.join("owner-retained.cause").exists());
}
fn command_of(pid: u32) -> String {
    let out = std::process::Command::new("/bin/ps")
        .args(["-o", "command=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}
/// The agent is the retained process running the scripted `claude`; the owner is the other
/// one, positively the parent of that agent, running the ctl binary. Anything else is
/// inconclusive.
fn identify(scratch: &Scratch, found: &[ProcessIdentity]) -> (ProcessIdentity, ProcessIdentity) {
    let shim = scratch.bin.join("claude").display().to_string();
    let agents: Vec<_> = found
        .iter()
        .filter(|i| command_of(i.pid()).contains(&shim))
        .collect();
    assert_eq!(agents.len(), 1, "inconclusive: agent among {found:?}");
    let agent = agents[0].clone();
    let parent = observe_caller(agent.pid(), deadline())
        .ok()
        .and_then(|o| o.chain.get(1).map(|l| l.identity.clone()))
        .expect("inconclusive: the agent's parent");
    let ctl = fs::canonicalize(env!("CARGO_BIN_EXE_harflow-ctl")).unwrap();
    let exe =
        observed_executable_path(&parent, deadline()).expect("inconclusive: owner executable");
    assert!(
        found.contains(&parent) && fs::canonicalize(exe).ok() == Some(ctl),
        "inconclusive: owner {parent:?} among {found:?}"
    );
    (parent, agent)
}
fn run_case(name: &str, steps: &str) {
    let scratch = Scratch::new(name);
    let gate = scratch.gate("a");
    // Armed before anything is launched: a timeout, a panic or an unknown outcome of the
    // first ctl keeps the scratch directory and prints its path.
    let mut guard = Guard::new(&scratch, &gate);
    let (code, stdout, stderr) = ctl(
        &scratch,
        steps,
        &run_args(&scratch, "Run the scripted task."),
    );
    if code != Some(0) {
        // A host that fails qualification refuses the run: refusal coverage only.
        assert_eq!(code, Some(1), "{stdout}{stderr}");
        assert!(
            stdout.contains("\"code\": \"invalid_params\""),
            "{stdout}{stderr}"
        );
        assert!(!scratch.root.exists(), "a refusal leaves no root");
        eprintln!("run refused by this host; refusal coverage only: {stdout}");
        guard.disarm();
        return;
    }
    let task = stdout
        .split("\"task_id\": \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_else(|| panic!("no task id in {stdout}"))
        .to_owned();
    // The agent waits at the gate before its result: all three processes are live.
    let found = recorded_processes(&scratch.root);
    found.iter().cloned().for_each(|i| guard.observe(i));
    assert_eq!(found.len(), 2, "owner and agent: {found:?}");
    let (owner, agent) = identify(&scratch, &found);
    let writer = writer_of(&owner, &agent);
    guard.observe(writer.clone());
    guard.unidentified = false;
    assert_ne!(writer, owner);
    assert!(guard.recorded().iter().all(alive));
    guard.release_gate();
    let status = wait_for("a completed task", Duration::from_secs(30), || {
        let (code, text, _) = ctl(&scratch, "", &["status", &task, "--json"]);
        (code == Some(0) && text.contains("\"status\": \"completed\"")).then_some(text)
    });
    assert!(
        status.contains("\"summary\": \"done\""),
        "the result evidence"
    );
    assert!(status.contains("\"exit_code\": 0"));
    assert_private_and_lossless(&scratch);
    let report = guard.finish(Duration::from_secs(10));
    assert!(
        report.killed.is_empty() && report.kept.is_empty(),
        "{report:?}"
    );
    assert_eq!(report.exited.len(), 3);
}

#[test]
fn ctl_run_unbuffered_lines_completes() {
    run_case(
        "burst",
        "read,init,burst:12,stderr:200,gate:a,result,eofwait,exit:0",
    );
}

#[test]
fn ctl_run_megabyte_burst_completes() {
    run_case("mib", "read,init,mib:1,gate:a,result,eofwait,exit:0");
}

#[test]
fn ctl_run_refuses_a_relative_repo_and_creates_no_root() {
    let scratch = Scratch::new("refusal");
    let mut args = run_args(&scratch, "x");
    let repo = args.iter().position(|a| a == "--repo").unwrap() + 1;
    args[repo] = "relative".into();
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, _) = ctl(&scratch, "read", &refs);
    assert_eq!(code, Some(1));
    assert!(stdout.contains("\"code\": \"invalid_params\""), "{stdout}");
    assert!(!scratch.root.exists(), "a refused run creates no root");
}

#[test]
fn an_armed_guard_dropped_with_a_broken_gate_keeps_the_scratch_and_does_not_panic() {
    let scratch = Scratch::new("guard");
    let gate = scratch.gate("a");
    let (ours, theirs) = std::os::unix::net::UnixStream::pair().unwrap();
    drop(theirs);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut guard = Guard::new(&scratch, &gate);
        guard.connection = Some(ours);
        // A recorded process that is already gone.
        let mut done = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        guard.observe(MacIdentity.identity(done.id()).unwrap());
        done.wait().unwrap();
        panic!("deliberate failure of the test body");
    }));
    // The unwind ran the guard's destructor: it neither panicked nor aborted.
    let payload = result.expect_err("the body panicked");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"deliberate failure of the test body"),
        "the caught payload is the original panic"
    );
    assert!(scratch.keep.get(), "an unidentified run keeps the scratch");
    scratch.keep.set(false);
}

#[test]
fn a_disarmed_guard_after_a_clean_refusal_releases_the_scratch() {
    let scratch = Scratch::new("disarm");
    let gate = scratch.gate("a");
    let mut guard = Guard::new(&scratch, &gate);
    assert!(scratch.keep.get());
    guard.disarm();
    assert!(!scratch.keep.get());
}

#[test]
fn cleanup_after_terminal_publication() {
    // Covered inside `run_case`: after the terminal, all three recorded identities exit
    // within ten seconds and nothing is killed. Here the same routine on processes that
    // exit by themselves.
    // Orphaned, so init reaps them and their exit is observable as `Dead`.
    let recorded: Vec<_> = (0..2)
        .map(|_| {
            let launched = std::process::Command::new("/bin/sh")
                .args(["-c", "/bin/sleep 1 >/dev/null 2>&1 </dev/null & echo $!"])
                .output()
                .unwrap();
            let pid: u32 = String::from_utf8(launched.stdout)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            wait_for("a short-lived process", Duration::from_secs(5), || {
                MacIdentity.identity(pid)
            })
        })
        .collect();
    let report = cleanup(&recorded, Duration::from_secs(10));
    assert!(
        report.killed.is_empty() && report.kept.is_empty(),
        "{report:?}"
    );
    assert_eq!(report.exited.len(), 2);
}

#[test]
fn cleanup_after_pre_t1_failure() {
    // A release ctl cannot inject a pre-T1 failure without seams, so a dummy process stands
    // in for the owner that never finished: it outlives the wait and is killed by identity.
    // Orphaned on purpose, so a killed process is reaped by init and its exit is observable.
    let launched = std::process::Command::new("/bin/sh")
        .args(["-c", "/bin/sleep 300 >/dev/null 2>&1 </dev/null & echo $!"])
        .output()
        .unwrap();
    let pid: u32 = String::from_utf8(launched.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let recorded = [wait_for("the dummy owner", Duration::from_secs(5), || {
        MacIdentity.identity(pid)
    })];
    let report = cleanup(&recorded, Duration::from_millis(300));
    assert_eq!(
        report,
        Report {
            killed: vec![recorded[0].pid()],
            ..Report::default()
        }
    );
}

#[test]
fn cleanup_keeps_identity_mismatch() {
    let mut other = std::process::Command::new("/bin/sleep")
        .arg("300")
        .spawn()
        .unwrap();
    let real = MacIdentity.identity(other.id()).unwrap();
    let stale = ProcessIdentity::new(
        real.pid(),
        real.start_seconds() + 1,
        real.start_microseconds(),
        real.boot_uuid().to_owned(),
    )
    .unwrap();
    let report = cleanup(&[stale], Duration::from_millis(300));
    let survived = other.try_wait().unwrap().is_none();
    other.kill().unwrap();
    let _ = other.wait();
    assert!(survived, "a mismatched record never signals the process");
    assert_eq!(report.kept, vec![real.pid()]);
    assert!(report.killed.is_empty() && report.exited.is_empty());
}

#[test]
fn only_a_dead_observation_confirms_an_exit() {
    assert_eq!(after_wait(Seen::Dead), Next::Exited);
    assert_eq!(after_wait(Seen::Matching), Next::Signal);
    assert_eq!(after_wait(Seen::Mismatch), Next::Kept);
    assert_eq!(
        after_wait(Seen::Unknown),
        Next::Kept,
        "a failed lookup is not a death"
    );
    assert!(signal_confirmed(true, Seen::Dead));
    for seen in [Seen::Matching, Seen::Mismatch, Seen::Unknown] {
        assert!(!signal_confirmed(true, seen), "{seen:?}");
    }
    assert!(
        !signal_confirmed(false, Seen::Dead),
        "an undelivered signal is not a kill"
    );
    // Waits continue while a process is live or exited-but-unreaped, and stop on a verdict.
    assert!(unsettled(Seen::Matching) && unsettled(Seen::Unknown));
    assert!(!unsettled(Seen::Dead) && !unsettled(Seen::Mismatch));
}
