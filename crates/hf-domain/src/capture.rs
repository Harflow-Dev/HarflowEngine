//! Trusted internal fixture observations, not operating-system proof.
use crate::{
    ownership::Fence,
    process::{ProcessExit, ProcessIdentity},
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureBinding {
    pub fence: Fence,
    pub child: ProcessIdentity,
    pub stdout: FileIdentity,
    pub stderr: FileIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureProgress {
    pub revision: u64,
    pub stdout: u64,
    pub stderr: u64,
    pub stdout_eof: bool,
    pub stderr_eof: bool,
    pub loss: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureSnapshot {
    pub binding: CaptureBinding,
    pub progress: CaptureProgress,
    pub exit: ProcessExit,
}
