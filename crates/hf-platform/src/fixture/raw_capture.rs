//! Qualified private raw capture. Filesystem calls are nonpreemptible; late or
//! uncertain writes never return publishable offsets. No reopening or repair.
use hf_domain::{capture::FileIdentity, fixture::Stage, ownership::Id};
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    time::Instant,
};
pub const FIXED_STDOUT: &[u8] = b"harflow fixture stdout\x00\xff\n";
pub const FIXED_STDERR: &[u8] = b"harflow fixture stderr\x00\xfe\n";
const CAP: u64 = 65536;
const CHUNK: usize = 16384;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncedOffsets {
    pub stdout: u64,
    pub stderr: u64,
}
use crate::task_runtime::raw::files::RawFile;
pub struct RawCapture {
    stdout: RawFile,
    stderr: RawFile,
    poisoned: bool,
}
impl RawCapture {
    /// Qualify both fixed sentinel slots before ownership claim or child launch.
    pub fn prepare_sentinels(root: &Path, operation: &Id, deadline: Instant) -> io::Result<()> {
        let parent = root.parent().ok_or_else(refused)?;
        for stream in ["stdout", "stderr"] {
            timely(deadline)?;
            let path = parent.join(format!("r03-b2c.{}.{stream}.lock", operation.as_str()));
            drop(
                crate::lease::shared()
                    .acquire(
                        &path,
                        hf_application::lease::LeaseMode::Exclusive,
                        true,
                        deadline,
                    )
                    .map_err(|_| refused())?,
            );
        }
        timely(deadline)
    }
    /// Deny-only diagnostic fills the existing retained descriptor pool.
    pub fn exhaust_fixture_capacity(root: &Path, deadline: Instant) -> io::Result<()> {
        let parent = root.parent().ok_or_else(refused)?;
        for index in 0..254 {
            timely(deadline)?;
            let path = parent.join(format!("r04-d.capacity.{index:03}.lock"));
            drop(
                crate::lease::shared()
                    .acquire(
                        &path,
                        hf_application::lease::LeaseMode::Exclusive,
                        true,
                        deadline,
                    )
                    .map_err(|_| refused())?,
            );
        }
        timely(deadline)
    }

    pub fn create(root: &Path, operation: &Id, deadline: Instant) -> io::Result<Self> {
        timely(deadline)?;
        let parent = root.parent().ok_or_else(refused)?;
        let name = format!("r03-b2c.{}", operation.as_str());
        let stdout = create_file(parent.join(format!("{name}.stdout")), deadline)?;
        let stderr = create_file(parent.join(format!("{name}.stderr")), deadline)?;
        if stdout.identity() == stderr.identity() {
            return Err(refused());
        }
        File::open(parent)?.sync_all()?;
        stdout.validate(0)?;
        stderr.validate(0)?;
        timely(deadline)?;
        Ok(Self {
            stdout,
            stderr,
            poisoned: false,
        })
    }
    pub fn identities(&self) -> (FileIdentity, FileIdentity) {
        (self.stdout.identity(), self.stderr.identity())
    }
    /// Requalifies the published raw extent before terminal publication.
    pub fn validate(&mut self, stdout: u64, stderr: u64, deadline: Instant) -> io::Result<()> {
        if self.poisoned {
            return Err(refused());
        }
        self.poisoned = true;
        timely(deadline)?;
        self.stdout.validate(stdout)?;
        self.stderr.validate(stderr)?;
        timely(deadline)?;
        self.poisoned = false;
        Ok(())
    }
    pub fn append(
        &mut self,
        stdout_expected: u64,
        stderr_expected: u64,
        stdout: &[u8],
        stderr: &[u8],
        deadline: Instant,
        mut hook: impl FnMut(Stage) -> io::Result<()>,
    ) -> io::Result<SyncedOffsets> {
        if self.poisoned {
            return Err(refused());
        }
        self.poisoned = true;
        let result = self.append_inner(
            stdout_expected,
            stderr_expected,
            stdout,
            stderr,
            deadline,
            &mut hook,
        );
        if result.is_ok() {
            self.poisoned = false;
        }
        result
    }
    fn append_inner(
        &mut self,
        stdout_expected: u64,
        stderr_expected: u64,
        stdout: &[u8],
        stderr: &[u8],
        deadline: Instant,
        hook: &mut impl FnMut(Stage) -> io::Result<()>,
    ) -> io::Result<SyncedOffsets> {
        timely(deadline)?;
        let out = extent(stdout_expected, stdout)?;
        let err = extent(stderr_expected, stderr)?;
        self.stdout.validate(stdout_expected)?;
        self.stderr.validate(stderr_expected)?;
        self.stdout.write_unflushed(stdout)?;
        self.stderr.write_unflushed(stderr)?;
        self.stdout.validate(out)?;
        self.stderr.validate(err)?;
        hook(Stage::CaptureBeforeSync)?;
        timely(deadline)?;
        self.stdout.sync()?;
        self.stderr.sync()?;
        self.stdout.validate(out)?;
        self.stderr.validate(err)?;
        hook(Stage::CaptureAfterSync)?;
        timely(deadline)?;
        self.stdout.validate(out)?;
        self.stderr.validate(err)?;
        timely(deadline)?;
        Ok(SyncedOffsets {
            stdout: out,
            stderr: err,
        })
    }
}
fn create_file(path: PathBuf, deadline: Instant) -> io::Result<RawFile> {
    let sentinel = path.with_extension(format!(
        "{}.lock",
        path.extension()
            .and_then(|v| v.to_str())
            .ok_or_else(refused)?
    ));
    RawFile::create_qualified(path, sentinel, CAP, false, deadline)
}
fn extent(expected: u64, bytes: &[u8]) -> io::Result<u64> {
    let next = expected
        .checked_add(bytes.len() as u64)
        .ok_or_else(refused)?;
    if bytes.len() > CHUNK || next > CAP {
        return Err(refused());
    }
    Ok(next)
}
fn timely(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "raw capture deadline",
        ))
    } else {
        Ok(())
    }
}
fn refused() -> io::Error {
    io::Error::other("raw capture refused")
}
