//! Scratch area, fake agent, bounded ctl runs and identity-checked cleanup.
#![allow(dead_code)]
use hf_application::process::IdentityPort;
use hf_domain::process::{IdentityObservation, ProcessIdentity};
use hf_platform::process::MacIdentity;
use std::{
    cell::Cell,
    fs,
    io::Write,
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const FAKE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/task_runtime/tests/fake_claude.py"
);
const CTL_LIMIT: Duration = Duration::from_secs(30);

pub struct Scratch {
    pub dir: PathBuf,
    pub root: PathBuf,
    pub bin: PathBuf,
    pub gates: PathBuf,
    /// Keeps the directory (and prints its path) when a process could not be accounted for.
    pub keep: Cell<bool>,
}
impl Scratch {
    pub fn new(name: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let base = fs::canonicalize(std::env::temp_dir()).unwrap();
        let dir = base.join(format!(
            "hf-rp-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        let (bin, gates) = (dir.join("bin"), dir.join("g"));
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&gates).unwrap();
        let shim = bin.join("claude");
        fs::copy(FAKE, &shim).unwrap();
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            root: dir.join("state"),
            dir,
            bin,
            gates,
            keep: Cell::new(false),
        }
    }
    pub fn gate(&self, name: &str) -> UnixListener {
        let listener = UnixListener::bind(self.gates.join(name)).unwrap();
        listener.set_nonblocking(true).unwrap();
        listener
    }
    pub fn out(&self) -> Vec<u8> {
        fs::read(self.dir.join("stdout.log")).unwrap_or_default()
    }
    pub fn err(&self) -> Vec<u8> {
        fs::read(self.dir.join("stderr.log")).unwrap_or_default()
    }
    /// The private raw payload directory of the only launch in the root.
    pub fn payload(&self) -> PathBuf {
        let mut path = self.root.join("private-payload");
        for _ in 0..3 {
            let mut entries: Vec<_> = fs::read_dir(&path)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            assert_eq!(entries.len(), 1, "{path:?}");
            path = entries.remove(0);
        }
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        if self.keep.get() {
            let _ = writeln!(
                std::io::stderr(),
                "kept scratch directory: {}",
                self.dir.display()
            );
        } else {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}
/// Accounts for every process a run started, also when the test panics: it records
/// identities as they are observed, releases the agent's gate and runs the identity-checked
/// cleanup on unwind. Declared after the scratch directory, so it drops first.
pub struct Guard<'a> {
    scratch: &'a Scratch,
    listener: &'a UnixListener,
    pub connection: Option<UnixStream>,
    recorded: Vec<ProcessIdentity>,
    /// True until every process of the run has been positively identified.
    pub unidentified: bool,
    done: bool,
}
impl<'a> Guard<'a> {
    /// Created before the run is launched: until it is disarmed the scratch is kept.
    pub fn new(scratch: &'a Scratch, listener: &'a UnixListener) -> Self {
        scratch.keep.set(true);
        Self {
            scratch,
            listener,
            connection: None,
            recorded: Vec::new(),
            unidentified: true,
            done: false,
        }
    }
    pub fn observe(&mut self, identity: ProcessIdentity) {
        if !self.recorded.contains(&identity) {
            self.recorded.push(identity);
        }
    }
    pub fn recorded(&self) -> &[ProcessIdentity] {
        &self.recorded
    }
    /// A proven clean refusal: nothing was started, so nothing is kept.
    pub fn disarm(&mut self) {
        self.done = true;
        self.scratch.keep.set(false);
    }
    /// Lets the agent past its gate: bounded and best effort, never panics.
    pub fn release_gate(&mut self) {
        let end = Instant::now() + Duration::from_secs(3);
        while self.connection.is_none() && Instant::now() < end {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    self.connection = Some(stream);
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        if let Some(stream) = &mut self.connection {
            let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
            let _ = stream.write_all(b"1");
        }
    }
    /// The identity-checked cleanup. The scratch stays kept unless every process was
    /// identified and accounted for: exited by itself or confirmed dead after a signal.
    pub fn finish(&mut self, within: Duration) -> Report {
        self.done = true;
        self.scratch.keep.set(true);
        let report = cleanup(&self.recorded, within);
        if report.kept.is_empty() && !self.unidentified {
            self.scratch.keep.set(false);
        }
        report
    }
}
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.release_gate();
            let report = self.finish(Duration::from_secs(10));
            let _ = writeln!(
                std::io::stderr(),
                "run_real_process cleanup after failure: {report:?}"
            );
        }
    }
}
/// A bounded `harflow-ctl` run with a cleared environment. On timeout only that direct
/// child is killed. Returns the exit code, stdout and stderr.
pub fn ctl<S: AsRef<str>>(
    scratch: &Scratch,
    steps: &str,
    args: &[S],
) -> (Option<i32>, String, String) {
    let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
    let (out, err) = (scratch.dir.join("ctl.out"), scratch.dir.join("ctl.err"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_harflow-ctl"))
        .args(&args)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", scratch.bin.display()))
        .env("HOME", &scratch.dir)
        .env("HARFLOW_STATE_ROOT", &scratch.root)
        .env("FAKE_STEPS", steps)
        .env("FAKE_VERSION", "2.1.322")
        .env("FAKE_GATES", &scratch.gates)
        .env("FAKE_LOG", scratch.dir.join("starts.log"))
        .env("FAKE_PIDFILE", scratch.dir.join("desc.pid"))
        .env("FAKE_OUTLOG", scratch.dir.join("stdout.log"))
        .env("FAKE_ERRLOG", scratch.dir.join("stderr.log"))
        .current_dir(&scratch.dir)
        .stdin(Stdio::null())
        .stdout(fs::File::create(&out).unwrap())
        .stderr(fs::File::create(&err).unwrap())
        .spawn()
        .unwrap();
    let end = Instant::now() + CTL_LIMIT;
    let code = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status.code();
        }
        if Instant::now() >= end {
            let _ = child.kill();
            let _ = child.wait();
            panic!("harflow-ctl {args:?} did not finish within {CTL_LIMIT:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let text = |path: &Path| fs::read_to_string(path).unwrap_or_default();
    (code, text(&out), text(&err))
}
pub fn run_args(scratch: &Scratch, prompt: &str) -> Vec<String> {
    [
        "run",
        "--backend",
        "claude",
        "--freedom",
        "unrestricted",
        "--repo",
    ]
    .iter()
    .map(ToString::to_string)
    .chain([
        scratch.dir.display().to_string(),
        "--prompt".into(),
        prompt.into(),
        "--json".into(),
    ])
    .collect()
}
pub fn observe(identity: &ProcessIdentity) -> IdentityObservation {
    MacIdentity.observe(identity)
}
pub fn alive(identity: &ProcessIdentity) -> bool {
    observe(identity) == IdentityObservation::Matching
}
/// Still to be decided: a live process, or one that exited but is not yet reaped (it
/// observes as `Unknown` until it is).
pub fn unsettled(observation: IdentityObservation) -> bool {
    matches!(
        observation,
        IdentityObservation::Matching | IdentityObservation::Unknown
    )
}
/// What to do with a recorded process once the wait for its own exit is over.
#[derive(Debug, Eq, PartialEq)]
pub enum Next {
    Exited,
    Signal,
    Kept,
}
/// Only a positive `Dead` observation is an exit; `Unknown` and `Mismatch` are kept.
pub fn after_wait(observation: IdentityObservation) -> Next {
    match observation {
        IdentityObservation::Dead => Next::Exited,
        IdentityObservation::Matching => Next::Signal,
        IdentityObservation::Mismatch | IdentityObservation::Unknown => Next::Kept,
    }
}
/// A signal counts only when it was delivered and the process is then observed `Dead`.
pub fn signal_confirmed(sent: bool, observation: IdentityObservation) -> bool {
    sent && observation == IdentityObservation::Dead
}
/// What cleanup did with the recorded identities.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Report {
    pub exited: Vec<u32>,
    pub killed: Vec<u32>,
    pub kept: Vec<u32>,
}
/// Waits for every recorded identity to exit. A survivor is re-observed just before a
/// SIGKILL and killed only while its pid and start time still match the record; anything
/// else is kept and reported, never killed.
pub fn cleanup(recorded: &[ProcessIdentity], within: Duration) -> Report {
    let mut report = Report::default();
    let end = Instant::now() + within;
    for identity in recorded {
        while unsettled(observe(identity)) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        match after_wait(observe(identity)) {
            Next::Exited => report.exited.push(identity.pid()),
            Next::Kept => report.kept.push(identity.pid()),
            Next::Signal => {
                let sent = Command::new("/bin/kill")
                    .args(["-KILL", &identity.pid().to_string()])
                    .status()
                    .is_ok_and(|status| status.success());
                let settle = Instant::now() + Duration::from_secs(2);
                while unsettled(observe(identity)) && Instant::now() < settle {
                    std::thread::sleep(Duration::from_millis(20));
                }
                if signal_confirmed(sent, observe(identity)) {
                    report.killed.push(identity.pid());
                } else {
                    report.kept.push(identity.pid());
                }
            }
        }
    }
    report
}
