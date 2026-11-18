//! The scripted agent CLI and its gates.
//!
//! The script is a python file installed as `claude` in a scratch directory. Its steps
//! come from the environment; a gate is a unix socket the test listens on, so a step that
//! must wait for the test is deterministic.
use std::{
    collections::HashMap,
    io::Write,
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const SCRIPT: &str = include_str!("fake_claude.py");

pub(super) struct Fake {
    pub(super) bin: PathBuf,
    pub(super) gates: PathBuf,
    pub(super) log: PathBuf,
    pub(super) pidfile: PathBuf,
}
impl Fake {
    pub(super) fn install(scratch: &Path) -> Self {
        let bin = scratch.join("bin");
        let gates = scratch.join("g");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&gates).unwrap();
        let script = bin.join("claude");
        std::fs::write(&script, SCRIPT).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let stub = bin.join("ctl-stub");
        std::fs::write(&stub, b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            log: scratch.join("starts.log"),
            pidfile: scratch.join("desc.pid"),
            bin,
            gates,
        }
    }
    /// The environment the dispatcher would capture, sorted as production sorts it.
    pub(super) fn environment(&self, steps: &str, version: &str) -> Vec<(String, String)> {
        let text = |path: &Path| path.to_str().unwrap().to_owned();
        let mut values = vec![
            ("PATH".to_owned(), text(&self.bin)),
            ("FAKE_STEPS".to_owned(), steps.to_owned()),
            ("FAKE_VERSION".to_owned(), version.to_owned()),
            ("FAKE_GATES".to_owned(), text(&self.gates)),
            ("FAKE_LOG".to_owned(), text(&self.log)),
            ("FAKE_PIDFILE".to_owned(), text(&self.pidfile)),
        ];
        values.sort();
        values
    }
    /// Agent starts, not counting version probes.
    pub(super) fn starts(&self) -> usize {
        std::fs::read_to_string(&self.log).map_or(0, |text| text.lines().count())
    }
    pub(super) fn descendant_pid(&self) -> Option<u32> {
        std::fs::read_to_string(&self.pidfile)
            .ok()?
            .trim()
            .parse()
            .ok()
    }
}
/// Listening gates; `open` releases the script's next wait on that gate.
pub(super) struct Gates {
    dir: PathBuf,
    listeners: HashMap<String, UnixListener>,
    held: HashMap<String, UnixStream>,
}
impl Gates {
    pub(super) fn new(fake: &Fake, names: &[&str]) -> Self {
        let mut listeners = HashMap::new();
        for name in names {
            let listener = UnixListener::bind(fake.gates.join(name)).unwrap();
            listener.set_nonblocking(true).unwrap();
            listeners.insert((*name).to_owned(), listener);
        }
        Self {
            dir: fake.gates.clone(),
            listeners,
            held: HashMap::new(),
        }
    }
    /// Waits for the script to reach the gate. The connection stays held until `close`.
    pub(super) fn connect(&mut self, name: &str, within: Duration) -> bool {
        let end = Instant::now() + within;
        let listener = self.listeners.get(name).unwrap();
        while Instant::now() < end {
            if let Ok((stream, _)) = listener.accept() {
                stream.set_nonblocking(false).unwrap();
                self.held.insert(name.to_owned(), stream);
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }
    pub(super) fn open(&mut self, name: &str) {
        if !self.held.contains_key(name) {
            assert!(
                self.connect(name, Duration::from_secs(20)),
                "gate {name} never reached"
            );
        }
        self.held.get_mut(name).unwrap().write_all(b"1").unwrap();
    }
    /// Drops the held connection: a waiting script sees end of file.
    pub(super) fn close(&mut self, name: &str) {
        self.held.remove(name);
    }
    pub(super) fn close_all(&mut self) {
        self.held.clear();
        self.listeners.clear();
        let _ = &self.dir;
    }
}
