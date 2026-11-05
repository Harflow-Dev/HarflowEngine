//! Qualified private raw capture. Filesystem calls are nonpreemptible; late or
//! uncertain writes never return publishable offsets. No reopening or repair.
use hf_domain::{capture::FileIdentity, fixture::Stage, ownership::Id};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
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
struct RawFile {
    _lease: Box<dyn hf_application::lease::LeaseGuard>,
    file: File,
    path: PathBuf,
    identity: FileIdentity,
}
pub struct RawCapture {
    stdout: RawFile,
    stderr: RawFile,
    poisoned: bool,
}
impl RawCapture {
    pub fn create(root: &Path, operation: &Id, deadline: Instant) -> io::Result<Self> {
        timely(deadline)?;
        let parent = root.parent().ok_or_else(refused)?;
        let name = format!("r03-b2c.{}", operation.as_str());
        let stdout = RawFile::create(parent.join(format!("{name}.stdout")), deadline)?;
        let stderr = RawFile::create(parent.join(format!("{name}.stderr")), deadline)?;
        if stdout.identity == stderr.identity {
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
        (self.stdout.identity.clone(), self.stderr.identity.clone())
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
        self.stdout.file.write_all(stdout)?;
        self.stderr.file.write_all(stderr)?;
        self.stdout.validate(out)?;
        self.stderr.validate(err)?;
        hook(Stage::CaptureBeforeSync)?;
        timely(deadline)?;
        self.stdout.file.sync_all()?;
        self.stderr.file.sync_all()?;
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
impl RawFile {
    fn create(path: PathBuf, deadline: Instant) -> io::Result<Self> {
        let sentinel = path.with_extension(format!(
            "{}.lock",
            path.extension()
                .and_then(|v| v.to_str())
                .ok_or_else(refused)?
        ));
        let lease = crate::lease::shared()
            .acquire(
                &sentinel,
                hf_application::lease::LeaseMode::Exclusive,
                true,
                deadline,
            )
            .map_err(|_| refused())?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(&path)?;

        let identity = identify(&file.metadata()?, 0)?;
        let value = Self {
            _lease: lease,
            file,
            path,
            identity,
        };
        value.validate(0)?;
        Ok(value)
    }
    fn validate(&self, expected: u64) -> io::Result<()> {
        if identify(&self.file.metadata()?, expected)? != self.identity
            || identify(&fs::symlink_metadata(&self.path)?, expected)? != self.identity
        {
            return Err(refused());
        }
        Ok(())
    }
}
fn identify(metadata: &fs::Metadata, length: u64) -> io::Result<FileIdentity> {
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.ino() == 0
        || metadata.len() != length
        || length > CAP
    {
        return Err(refused());
    }
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
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
