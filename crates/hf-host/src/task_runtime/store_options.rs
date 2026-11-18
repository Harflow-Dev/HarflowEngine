//! Writer options of the host's store handles: the checkpoint policy each role names,
//! with the platform flush for `DEFAULT_DURABILITY` as the commit-watermark flush.
use hf_platform::durability::{DEFAULT_DURABILITY, port};
use hf_storage::WriterOptions;

/// `WriterOptions::suppressed()` with the default durability: the dispatcher, the owner's
/// validation open, readers, watchers and fixtures never checkpoint.
pub fn suppressed() -> WriterOptions {
    WriterOptions::suppressed().with_flush(port(DEFAULT_DURABILITY))
}

/// `WriterOptions::owner_idle()` with the default durability: the task writer checkpoints
/// from its idle loop and bounds `-wal` for the lifetime of its handle.
pub fn writer() -> WriterOptions {
    WriterOptions::owner_idle().with_flush(port(DEFAULT_DURABILITY))
}
