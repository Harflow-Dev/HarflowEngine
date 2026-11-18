//! Detached original owner: one child waiter, independent reader and original writer.
use super::{Error, OwnerEntry, WriterLink, io};
use hf_domain::task_runtime::*;
use hf_platform::{
    backend_launch::PathClaudeDiscovery,
    process::{CaptureOrigin, CaptureReader, RetainedOwnedChild},
};
use std::{path::PathBuf, time::Duration};
pub(super) struct Runtime {
    pub(super) entry: OwnerEntry,
    pub(super) writer: Option<WriterLink>,
    pub(super) agent: Option<RetainedOwnedChild>,
    pub(super) reader: Option<CaptureReader>,
    pub(super) discovery: Option<PathClaudeDiscovery<hf_storage::Sha256Digest>>,
    pub(super) envelope: Option<WriterEnvelope>,
    pub(super) origin: Option<CaptureOrigin>,
    pub(super) intent: Option<LaunchIntentCreate>,
    pub(super) input_bytes: u64,
    pub(super) stdout_end: u64,
    pub(super) stderr_end: u64,
    pub(super) input_closed: bool,
    pub(super) source_binding: Option<hf_domain::ownership::Digest>,
}
impl Runtime {
    fn new(entry: OwnerEntry) -> Self {
        Self {
            entry,
            writer: None,
            agent: None,
            reader: None,
            discovery: None,
            envelope: None,
            origin: None,
            intent: None,
            input_bytes: 0,
            stdout_end: 0,
            stderr_end: 0,
            input_closed: false,
            source_binding: None,
        }
    }
}

pub fn run_owner(root: PathBuf) -> i32 {
    let entry = match OwnerEntry::enter(&root, io::deadline()) {
        Ok(entry) => entry,
        Err(_) => return 1,
    };
    let mut runtime = Runtime::new(entry);
    drive(&mut runtime)
}

/// Runs an initialized owner to settlement; the runtime stays owned by the caller.
pub(super) fn drive(runtime: &mut Runtime) -> i32 {
    match runtime.initialize() {
        Ok(()) => (),
        Err(Error::NotStarted(_)) => return 0,
        Err(_) => runtime.retain_uncertain(),
    }
    loop {
        match runtime.tick() {
            Ok(true) => break,
            Ok(false) => std::thread::sleep(Duration::from_millis(1)),
            Err(_) => runtime.retain_uncertain(),
        }
    }
    if runtime.settle_writer().is_err() {
        runtime.retain_uncertain();
    }
    0
}
