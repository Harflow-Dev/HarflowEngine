use super::oracles::{network_receipt, read_helper, witness_unchanged};
use super::{CAPTURE_LIMIT, ProbeControl, absence_profile};
use crate::process::{Capture, RetainedOwnedChild, SpawnInitialization, SpawnSpec};
use crate::qualification::FixtureRoot;
use hf_application::process::IdentityPort;
use hf_domain::process::{ProcessExit, ProcessIdentity};
use std::ffi::OsString;
use std::io::{self, Write};
use std::net::TcpListener;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct HelperLaunch {
    root: FixtureRoot,
    helper: PathBuf,
    profile: String,
    bytes: Vec<u8>,
    observer: TcpListener,
    witness: PathBuf,
    controller: Option<ProcessIdentity>,
    cwd: PathBuf,
}
impl HelperLaunch {
    /// Does not compile or execute. Caller must review exact source/build/profile
    /// hashes before launch. Byte comparison does not eliminate executable TOCTOU.
    pub fn prepare(root: FixtureRoot, helper: PathBuf) -> io::Result<Self> {
        root.validate_topology()?;
        let profile = absence_profile(&helper)?;
        let bytes = read_helper(&helper)?;
        let witness = root.workspace().join("st04-read-witness");
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&witness)?
            .write_all(b"ST04-READ-WITNESS\n")?;
        let observer = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        observer.set_nonblocking(true)?;
        Ok(Self {
            root,
            helper,
            profile,
            bytes,
            observer,
            witness,
            controller: None,
            cwd: std::env::current_dir()?,
        })
    }
    /// Single-permission control; never a qualifying absence profile.
    pub fn prepare_control(
        root: FixtureRoot,
        helper: PathBuf,
        control: ProbeControl,
    ) -> io::Result<Self> {
        let mut launch = Self::prepare(root, helper)?;
        launch.profile = super::profile::controlled_profile(
            &launch.helper,
            launch.root.root(),
            launch.observer.local_addr()?.port(),
            control,
        )?;
        Ok(launch)
    }
    /// Opt-in actual SIGCONT probe is restricted to the dedicated fixture test binary.
    pub fn enable_controller_signal_probe(&mut self) -> io::Result<ProcessIdentity> {
        let executable = std::env::current_exe()?;
        if !executable
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("st04_helpers-"))
        {
            return Err(io::Error::other(
                "dedicated st04_helpers controller required",
            ));
        }
        let identity = crate::process::MacIdentity
            .identity(std::process::id())
            .ok_or_else(|| io::Error::other("controller identity unknown"))?;
        self.controller = Some(identity.clone());
        Ok(identity)
    }
    pub fn profile(&self) -> &str {
        &self.profile
    }
    pub fn root(&self) -> &FixtureRoot {
        &self.root
    }
    /// Exact prepared invocation for pre-issuance manifests; also used by launch.
    /// Call after selecting controller signal mode. This accessor never executes.
    pub fn spawn_spec(&self) -> io::Result<SpawnSpec> {
        Ok(SpawnSpec {
            executable: PathBuf::from("/usr/bin/sandbox-exec"),
            cwd: self.cwd.clone(),
            args: vec![
                OsString::from("-p"),
                self.profile.clone().into(),
                self.helper.clone().into_os_string(),
                self.root.workspace().join("st04-effect").into_os_string(),
                self.witness.clone().into_os_string(),
                self.observer.local_addr()?.port().to_string().into(),
                std::process::id().to_string().into(),
                if self.controller.is_some() {
                    "signal".into()
                } else {
                    "check".into()
                },
            ],
            environment: Vec::new(),
            capture_limit: CAPTURE_LIMIT,
        })
    }
    pub fn launch(
        self,
        source: &impl IdentityPort,
        deadline: Instant,
    ) -> io::Result<HelperMeasurement> {
        self.root.validate_topology()?;
        if let Some(controller) = &self.controller
            && source.identity(std::process::id()).as_ref() != Some(controller)
        {
            return Err(io::Error::other("controller identity changed"));
        }
        if read_helper(&self.helper)? != self.bytes {
            return Err(io::Error::other("helper changed before issuance"));
        }
        let probe = self.root.workspace().join("st04-effect");
        if probe.try_exists()? {
            return Err(io::Error::other("probe already exists"));
        }
        let spec = self.spawn_spec()?;
        let child = RetainedOwnedChild::spawn(spec, source, deadline)?;
        Ok(HelperMeasurement {
            root: self.root,
            child,
            probe,
            poisoned: false,
            released: false,
            observer: self.observer,
            witness: self.witness,
        })
    }
}
/// Original sole waiter and fixture root remain available after every error.
pub struct HelperMeasurement {
    root: FixtureRoot,
    child: RetainedOwnedChild,
    probe: PathBuf,
    poisoned: bool,
    released: bool,
    observer: TcpListener,
    witness: PathBuf,
}
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub identity: Option<ProcessIdentity>,
    pub exit: ProcessExit,
    pub capture: Capture,
    pub probe_absent: bool,
    pub network_observed: bool,
    pub network_bytes: Vec<u8>,
    pub settled: bool,
    /// Full OS absence qualification always remains unsupported in this slice.
    pub absence_qualified: bool,
}
impl HelperMeasurement {
    pub fn root(&self) -> &FixtureRoot {
        &self.root
    }
    pub fn child_mut(&mut self) -> &mut RetainedOwnedChild {
        &mut self.child
    }
    pub fn release(&mut self, deadline: Instant) -> io::Result<()> {
        let result = self.release_once(deadline);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn release_once(&mut self, deadline: Instant) -> io::Result<()> {
        if self.released
            || self.poisoned
            || self.child.initialization() != SpawnInitialization::Ready
        {
            return Err(io::Error::other("release refused"));
        }
        let deadline = deadline.min(Instant::now() + Duration::from_secs(4));
        while self.child.capture().stdout.bytes.len() < 11 && Instant::now() < deadline {
            self.child.pump(deadline, 8192)?;
        }
        if self.child.capture().stdout.bytes != b"ST04-READY\n" {
            return Err(io::Error::other("readiness unknown"));
        }
        // One issuance only, including an ambiguous write or missing acknowledgement.
        self.released = true;
        if self.child.write_stdin(b"R", deadline)? != 1 {
            return Err(io::Error::other("release delivery unknown"));
        }
        self.child.close_stdin();
        Ok(())
    }
    pub fn settle(&mut self, deadline: Instant) -> io::Result<Diagnostic> {
        let result = self.observe(deadline);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn observe(&mut self, deadline: Instant) -> io::Result<Diagnostic> {
        let deadline = deadline.min(Instant::now() + Duration::from_secs(10));
        loop {
            self.child.try_wait()?;
            let capture = self.child.capture();
            if self.child.exit() != ProcessExit::Unobserved
                && capture.stdout.eof
                && capture.stderr.eof
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other("settlement unknown; retain root"));
            }
            self.child.pump(deadline, 8192)?;
        }
        self.root.validate_topology()?;
        let absent = match std::fs::symlink_metadata(&self.probe) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => true,
            Ok(meta) if meta.is_file() && meta.nlink() == 1 => false,
            _ => return Err(io::Error::other("probe oracle unknown")),
        };
        let receipt = network_receipt(&self.observer, deadline)?;
        let network_absent = receipt.is_none();
        let witness_unchanged = witness_unchanged(&self.witness)?;
        let capture = self.child.capture().clone();
        let settled = !self.poisoned
            && self.released
            && self.child.exit() == ProcessExit::Code(0)
            && !capture.stdout.overflow
            && !capture.stderr.overflow
            && capture.stdout.error.is_none()
            && capture.stderr.error.is_none()
            && capture.stdout.bytes == b"ST04-READY\nST04-WRITE-DENIED\nST04-READ-DENIED\nST04-FORK-DENIED\nST04-SPAWN-DENIED\nST04-NETWORK-DENIED\nST04-SIGNAL-DENIED\n"
            && capture.stderr.bytes == b"ST04-STDERR\n"
            && absent
            && network_absent
            && witness_unchanged;
        Ok(Diagnostic {
            identity: self.child.registered_identity().cloned(),
            exit: self.child.exit(),
            capture,
            probe_absent: absent,
            network_observed: !network_absent,
            network_bytes: receipt.unwrap_or_default(),
            settled,
            absence_qualified: false,
        })
    }
}
