//! Same-release task composition, independent of the diagnostic fixture runtime.
mod binding;
mod bootstrap;
mod owner_entry;
mod transfer;
mod writer;
mod writer_entry;
mod writer_link;
pub use binding::{configuration_digest, invocation_arguments, invocation_digest, timestamp};
mod environment;
mod io;
mod stream;
#[cfg(test)]
mod stream_tests;
mod task_projection;
pub use environment::EnvironmentSender;
mod not_started;
mod writer_runtime;
pub use writer_runtime::run_writer;
mod dispatch;
mod dispatch_child;
mod dispatch_request;
pub use dispatch::run_for_ctl_exit;
mod owner_loop;
mod owner_runtime;
pub use bootstrap::{HeldRoot, prepare};
pub use owner_entry::OwnerEntry;
pub use owner_runtime::run_owner;
pub use transfer::{IntentReceiver, IntentSender};
pub use writer::Writer;
pub use writer_entry::WriterEntry;
pub use writer_link::{StderrDrain, WriterDrain, WriterLink};

/// A failed or late mutation is never replayed by the host.
#[derive(Debug)]
pub enum Error {
    Refused,
    NotStarted(String),
    Poisoned,
    Storage(hf_storage::Error),
    Io(std::io::Error),
}
impl From<hf_storage::Error> for Error {
    fn from(error: hf_storage::Error) -> Self {
        Self::Storage(error)
    }
}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
