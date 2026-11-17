//! Same-release task composition, independent of the diagnostic fixture runtime.
mod bootstrap;
mod owner_entry;
mod transfer;
mod writer;
mod writer_entry;
mod writer_link;
pub use bootstrap::{HeldRoot, prepare};
pub use owner_entry::OwnerEntry;
pub use transfer::{IntentReceiver, IntentSender};
pub use writer::Writer;
pub use writer_entry::WriterEntry;
pub use writer_link::{StderrDrain, WriterDrain, WriterLink};

/// A failed or late mutation is never replayed by the host.
#[derive(Debug)]
pub enum Error {
    Refused,
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
