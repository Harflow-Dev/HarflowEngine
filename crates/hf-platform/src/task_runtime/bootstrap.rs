//! Fresh single-thread same-binary bootstrap over inherited private pipes.
mod descriptors;
use crate::process::{MacIdentity, current_parent_identity, observed_executable_path};
pub(crate) use descriptors::check_empty_entry;
use descriptors::{PipeIdentity, pipe_identity};
use hf_application::{prepared_launch::StreamingDigestFactory, process::IdentityPort};
use hf_domain::{prepared_launch::BinaryPin, process::ProcessIdentity};
use std::{fs::File, io, os::fd::AsFd, path::PathBuf, time::Instant};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootstrapObservation {
    pub process: ProcessIdentity,
    pub parent: ProcessIdentity,
    pub executable: PathBuf,
    pub parent_executable: PathBuf,
}
pub struct InheritedStdio {
    input: File,
    output: File,
    error: File,
    pipes: [PipeIdentity; 3],
    observed: BootstrapObservation,
}
fn refused() -> io::Error {
    io::Error::other("inherited bootstrap qualification refused")
}
fn observe(deadline: Instant) -> io::Result<BootstrapObservation> {
    timely(deadline)?;
    let process = MacIdentity
        .identity(std::process::id())
        .ok_or_else(refused)?;
    let parent = current_parent_identity().ok_or_else(refused)?;
    let executable = observed_executable_path(&process, deadline)?;
    let parent_executable = observed_executable_path(&parent, deadline)?;
    if MacIdentity.identity(process.pid()).as_ref() != Some(&process)
        || current_parent_identity().as_ref() != Some(&parent)
    {
        return Err(refused());
    }
    timely(deadline)?;
    Ok(BootstrapObservation {
        process,
        parent,
        executable,
        parent_executable,
    })
}
impl InheritedStdio {
    /// Must run before threads, stores, root holds, or other descriptor acquisition.
    /// Safe clones own only these pipe endpoints; no raw-fd ownership reconstruction.
    pub fn acquire(deadline: Instant) -> io::Result<Self> {
        timely(deadline)?;
        check_empty_entry(true)?;
        let pipes = [
            pipe_identity(&std::io::stdin())?,
            pipe_identity(&std::io::stdout())?,
            pipe_identity(&std::io::stderr())?,
        ];
        if pipes[0] == pipes[1] || pipes[0] == pipes[2] || pipes[1] == pipes[2] {
            return Err(refused());
        }
        let observed = observe(deadline)?;
        let input = File::from(std::io::stdin().as_fd().try_clone_to_owned()?);
        let output = File::from(std::io::stdout().as_fd().try_clone_to_owned()?);
        let error = File::from(std::io::stderr().as_fd().try_clone_to_owned()?);
        let value = Self {
            input,
            output,
            error,
            pipes,
            observed,
        };
        value.revalidate(deadline)?;
        Ok(value)
    }
    /// Setup-only bounded invitation read. Qualification remains available afterward.
    pub fn read_frame(&self, deadline: Instant, max_frame: usize) -> io::Result<Vec<u8>> {
        if !(6..=16384).contains(&max_frame) {
            return Err(refused());
        }
        self.revalidate(deadline)?;
        let frame = super::channel::blocking::read_frame(&self.input, deadline, max_frame)?;
        self.revalidate(deadline)?;
        Ok(frame)
    }
    pub fn observation(&self) -> &BootstrapObservation {
        &self.observed
    }
    /// Identity is observed independently of invitation bytes. This checks the current
    /// executable file pin; it does not claim a general mapped-image attestation.
    pub fn qualify(
        &self,
        expected_parent: &ProcessIdentity,
        ctl: &BinaryPin,
        hash: &impl StreamingDigestFactory,
        deadline: Instant,
    ) -> io::Result<()> {
        self.revalidate(deadline)?;
        if &self.observed.parent != expected_parent
            || self.observed.executable != std::path::Path::new(&ctl.physical_path)
            || self.observed.parent_executable != std::path::Path::new(&ctl.physical_path)
        {
            return Err(refused());
        }
        let mut expected = ctl.clone();
        expected.logical_path = expected.physical_path.clone();
        for path in [&self.observed.executable, &self.observed.parent_executable] {
            let actual = crate::backend_launch::observe_binary_pin(path, hash, deadline)
                .map_err(|_| refused())?;
            if actual != expected {
                return Err(refused());
            }
        }
        self.revalidate(deadline)
    }
    pub fn revalidate(&self, deadline: Instant) -> io::Result<()> {
        timely(deadline)?;
        if observe(deadline)? != self.observed {
            return Err(refused());
        }
        let retained = [
            pipe_identity(&self.input)?,
            pipe_identity(&self.output)?,
            pipe_identity(&self.error)?,
        ];
        let inherited = [
            pipe_identity(&std::io::stdin())?,
            pipe_identity(&std::io::stdout())?,
            pipe_identity(&std::io::stderr())?,
        ];
        if retained != self.pipes || inherited != self.pipes {
            return Err(refused());
        }
        timely(deadline)
    }
    pub fn into_channel(self, cap: usize) -> io::Result<(super::channel::Channel, File)> {
        super::channel::Channel::new(self.input, self.output, cap)
            .map(|channel| (channel, self.error))
    }
}
fn timely(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "bootstrap deadline",
        ))
    } else {
        Ok(())
    }
}
