//! Original writer-owned raw source. Uncertain writes poison permanently; never reopen.
pub(crate) mod files;
use files::{RawFile, RootPath};
use hf_application::task_lifecycle::{
    CaptureAdmissionRefused, CaptureProgress, DurableCaptureAdmission, DurableCaptureRequest,
};
use hf_domain::{process::ProcessIdentity, task_lifecycle::LifecycleBinding};
use std::{io, path::Path, time::Instant};
pub const RAW_CAP: u64 = 64 * 1024 * 1024;
pub const APPEND_CAP: usize = 16384;
/// Host wrapper owns the real storage RootHold and checks its original binding.
pub trait RawRootHold {
    fn validate(&self, binding: &LifecycleBinding) -> io::Result<()>;
}
pub struct RawCapture {
    hold: Box<dyn RawRootHold>,
    root: RootPath,
    binding: LifecycleBinding,
    child: ProcessIdentity,
    stdout: RawFile,
    stderr: RawFile,
    loss: RawFile,
    synced: CaptureProgress,
    published: CaptureProgress,
    poisoned: bool,
}
pub struct PreparedRawCapture {
    hold: Box<dyn RawRootHold>,
    root: RootPath,
    binding: LifecycleBinding,
    stdout: RawFile,
    stderr: RawFile,
    loss: RawFile,
}
impl PreparedRawCapture {
    /// Pure one-use binding to the actual retained original child after spawn.
    pub fn bind(self, child: ProcessIdentity) -> RawCapture {
        let zero = CaptureProgress {
            stdout_end: 0,
            stderr_end: 0,
            known_loss: false,
        };
        RawCapture {
            hold: self.hold,
            root: self.root,
            binding: self.binding,
            child,
            stdout: self.stdout,
            stderr: self.stderr,
            loss: self.loss,
            synced: zero,
            published: zero,
            poisoned: false,
        }
    }
}
/// Immutable, I/O-free transaction admission derived only from the retained raw source.
pub struct SyncedCaptureView {
    binding: LifecycleBinding,
    child: ProcessIdentity,
    previous: CaptureProgress,
    next: CaptureProgress,
    _sources: (
        hf_domain::capture::FileIdentity,
        hf_domain::capture::FileIdentity,
    ),
}
impl DurableCaptureAdmission for SyncedCaptureView {
    fn admit(&self, value: &DurableCaptureRequest<'_>) -> Result<(), CaptureAdmissionRefused> {
        if value.binding != &self.binding
            || value.child != &self.child
            || value.previous != self.previous
            || value.next != self.next
        {
            return Err(CaptureAdmissionRefused);
        }
        Ok(())
    }
}
impl RawCapture {
    /// Prepare before provider/child effects so construction refusal cannot abandon custody.
    pub fn prepare(
        root: &Path,
        hold: Box<dyn RawRootHold>,
        binding: LifecycleBinding,
        deadline: Instant,
    ) -> io::Result<PreparedRawCapture> {
        hold.validate(&binding)?;
        let root = RootPath::create(root, &binding, deadline)?;
        let stdout = RawFile::create(root.path().join("stdout.raw"), deadline)?;
        let stderr = RawFile::create(root.path().join("stderr.raw"), deadline)?;
        let loss = RawFile::create(root.path().join("loss.raw"), deadline)?;
        root.sync()?;
        root.validate()?;
        hold.validate(&binding)?;
        timely(deadline)?;
        Ok(PreparedRawCapture {
            hold,
            root,
            binding,
            stdout,
            stderr,
            loss,
        })
    }
    pub fn progress(&self) -> CaptureProgress {
        self.synced
    }
    /// Original open raw source identities; no pathname reopening or inferred EOF.
    pub fn source_identities(
        &self,
    ) -> (
        hf_domain::capture::FileIdentity,
        hf_domain::capture::FileIdentity,
    ) {
        (self.stdout.identity(), self.stderr.identity())
    }
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }
    /// Returns no view on any partial, late or uncertain append; all handles stay owned.
    pub fn append(
        &mut self,
        out: &[u8],
        err: &[u8],
        deadline: Instant,
    ) -> io::Result<SyncedCaptureView> {
        if self.poisoned {
            return Err(refused());
        }
        let next_out = self
            .synced
            .stdout_end
            .checked_add(out.len() as u64)
            .ok_or_else(refused)?;
        let next_err = self
            .synced
            .stderr_end
            .checked_add(err.len() as u64)
            .ok_or_else(refused)?;
        if out.len() > APPEND_CAP
            || err.len() > APPEND_CAP
            || next_out
                .checked_add(next_err)
                .is_none_or(|sum| sum > RAW_CAP)
        {
            return Err(refused());
        }
        self.poisoned = true;
        self.validate(deadline)?;
        self.stdout.append(self.synced.stdout_end, out, deadline)?;
        self.stderr.append(self.synced.stderr_end, err, deadline)?;
        self.hold.validate(&self.binding)?;
        self.root.validate()?;
        timely(deadline)?;
        self.synced.stdout_end = next_out;
        self.synced.stderr_end = next_err;
        self.poisoned = false;
        self.view(deadline)
    }
    /// Durably records that capture bytes were read but never delivered. The writer calls it
    /// when a settlement reports a reader queue overflow, before the terminal is published;
    /// a failure poisons the capture and no terminal follows. It never gates drainage.
    pub fn record_known_loss(&mut self, deadline: Instant) -> io::Result<SyncedCaptureView> {
        if self.poisoned {
            return Err(refused());
        }
        if !self.synced.known_loss {
            self.poisoned = true;
            self.validate(deadline)?;
            self.loss.append(0, b"capture-known-loss\n", deadline)?;
            self.hold.validate(&self.binding)?;
            self.root.validate()?;
            timely(deadline)?;
            self.synced.known_loss = true;
            self.poisoned = false;
        }
        self.view(deadline)
    }
    pub fn view(&mut self, deadline: Instant) -> io::Result<SyncedCaptureView> {
        if self.poisoned {
            return Err(refused());
        }
        self.poisoned = true;
        self.validate(deadline)?;
        self.poisoned = false;
        Ok(SyncedCaptureView {
            binding: self.binding.clone(),
            child: self.child.clone(),
            previous: self.published,
            next: self.synced,
            _sources: (self.stdout.identity(), self.stderr.identity()),
        })
    }
    /// Host calls only after a positively matched storage acknowledgement. A lost or
    /// uncertain acknowledgement permanently poisons instead; it never permits replay.
    pub fn acknowledge(&mut self, view: &SyncedCaptureView) -> io::Result<()> {
        if self.poisoned
            || view.binding != self.binding
            || view.child != self.child
            || view.previous != self.published
            || view.next != self.synced
        {
            self.poisoned = true;
            return Err(refused());
        }
        self.published = self.synced;
        Ok(())
    }
    pub fn poison(&mut self) {
        self.poisoned = true;
    }
    fn validate(&self, deadline: Instant) -> io::Result<()> {
        timely(deadline)?;
        self.hold.validate(&self.binding)?;
        self.root.validate()?;
        self.stdout.validate(self.synced.stdout_end)?;
        self.stderr.validate(self.synced.stderr_end)?;
        self.loss
            .validate(if self.synced.known_loss { 19 } else { 0 })?;
        timely(deadline)
    }
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
