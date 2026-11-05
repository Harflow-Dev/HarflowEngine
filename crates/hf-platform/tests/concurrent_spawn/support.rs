use hf_domain::process::ProcessExit;
use hf_platform::process::{MacIdentity, OwnedSignal, RetainedOwnedChild, SpawnSpec, WaitOutcome};
use std::{
    ffi::OsString,
    fs,
    io::{BufReader, Read, Write},
    path::PathBuf,
    process::{Child as OsChild, ChildStdin, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
pub(super) const PYTHON: &str = "/usr/bin/python3";
pub(super) const HELPER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/concurrent_spawn/helper.py"
);

pub(super) fn deadline(ms: u64) -> Instant {
    Instant::now() + Duration::from_millis(ms)
}

pub(super) fn read_small(path: &std::path::Path) -> String {
    let file = fs::File::open(path).unwrap();
    read_bounded(file)
}

fn read_bounded(reader: impl Read) -> String {
    let mut bytes = Vec::new();
    reader.take(4097).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 4096, "test helper output exceeded 4 KiB");
    String::from_utf8(bytes).unwrap()
}

pub(super) struct Temp(pub(super) PathBuf);
impl Temp {
    pub(super) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = PathBuf::from("/private/tmp").join(format!(
            "hf-concurrent-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn spec(args: &[&str]) -> SpawnSpec {
    let cwd = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
    SpawnSpec {
        executable: PathBuf::from(PYTHON),
        cwd,
        args: std::iter::once(OsString::from(HELPER))
            .chain(args.iter().map(OsString::from))
            .collect(),
        environment: Vec::new(),
        capture_limit: 65_536,
    }
}

pub(super) fn spawn(args: &[&str]) -> RetainedOwnedChild {
    RetainedOwnedChild::spawn(spec(args), &MacIdentity, deadline(2_000)).unwrap()
}

pub(super) fn wait_exit(child: &mut RetainedOwnedChild) {
    assert!(matches!(
        child.wait_until(deadline(3_000), || false).unwrap(),
        WaitOutcome::Exited(ProcessExit::Code(0))
    ));
}

pub(super) fn external_lock_probe(path: &std::path::Path) -> bool {
    let mut child = Command::new(PYTHON)
        .arg(HELPER)
        .arg("probe")
        .arg("exclusive")
        .arg(path)
        .current_dir(fs::canonicalize(std::env::current_dir().unwrap()).unwrap())
        .env_clear()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let end = deadline(1_000);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "independent Python probe failed");
            let output = read_bounded(child.stdout.take().unwrap());
            return output.trim() == "blocked";
        }
        if Instant::now() >= end {
            kill_bounded(&mut child);
            panic!("independent Python lock probe timed out");
        }
        std::hint::spin_loop();
    }
}

pub(super) struct Probe {
    child: OsChild,
    input: Option<ChildStdin>,
    output: BufReader<std::process::ChildStdout>,
}
impl Probe {
    pub(super) fn start(path: &std::path::Path) -> Self {
        let mut child = Command::new(PYTHON)
            .arg(HELPER)
            .arg("monitor")
            .arg(path)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let ready_path = path.with_file_name(format!(
            "{}.ready",
            path.file_name().unwrap().to_string_lossy()
        ));
        let end = deadline(2_000);
        loop {
            if ready_path.exists() {
                break;
            }
            if let Some(status) = child.try_wait().unwrap() {
                panic!("monitor exited before ready: {status}");
            }
            if Instant::now() >= end {
                kill_bounded(&mut child);
                panic!("monitor readiness deadline expired");
            }
            std::hint::spin_loop();
        }
        Self {
            child,
            input: Some(input),
            output,
        }
    }
    pub(super) fn finish(mut self) -> (u64, u64) {
        writeln!(self.input.as_mut().unwrap(), "stop").unwrap();
        self.input.take();
        let end = deadline(2_000);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                let text = read_bounded(&mut self.output);
                let values: Vec<&str> = text.split_whitespace().collect();
                assert_eq!(values.len(), 3);
                assert_eq!(
                    values[2], "stop",
                    "external monitor ended without explicit stop"
                );
                return (values[0].parse().unwrap(), values[1].parse().unwrap());
            }
            if Instant::now() >= end {
                kill_bounded(&mut self.child);
                panic!("external monitor exceeded its deadline");
            }
            std::hint::spin_loop();
        }
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            kill_bounded(&mut self.child);
        }
    }
}

fn kill_bounded(child: &mut OsChild) {
    let _ = child.kill();
    let end = deadline(500);
    while Instant::now() < end {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::hint::spin_loop();
    }
    std::process::abort();
}

pub(super) struct Child(pub(super) RetainedOwnedChild, bool);
impl Child {
    pub(super) fn new(process: RetainedOwnedChild) -> Self {
        Self(process, false)
    }
    pub(super) fn mark_external_settled(&mut self) {
        self.1 = true;
    }
}
impl Drop for Child {
    fn drop(&mut self) {
        if self.1 {
            return;
        }
        let _ = self.0.write_stdin(b"release\n", deadline(200));
        self.0.close_stdin();
        let end = deadline(500);
        while Instant::now() < end {
            if matches!(
                self.0.try_wait(),
                Ok(ProcessExit::Code(_) | ProcessExit::Signal(_))
            ) {
                return;
            }
            std::hint::spin_loop();
        }
        if let Some(identity) = self.0.registered_identity().cloned() {
            let _ =
                self.0
                    .request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(200));
            if !matches!(
                self.0.wait_until(deadline(1_000), || false),
                Ok(WaitOutcome::Exited(_))
            ) {
                std::process::abort();
            }
        } else {
            std::process::abort();
        }
    }
}

pub(super) struct ExternalChild(pub(super) OsChild);
impl Drop for ExternalChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_some() {
            return;
        }
        let _ = self.0.kill();
        let end = deadline(500);
        while Instant::now() < end {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            std::hint::spin_loop();
        }
        std::process::abort();
    }
}
