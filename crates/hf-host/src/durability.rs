//! Writer options of the host's store handles: the checkpoint policy each site names,
//! with the platform flush for `DEFAULT_DURABILITY` as the commit-watermark flush.
use hf_platform::durability::{DEFAULT_DURABILITY, port};
use hf_storage::WriterOptions;

/// `WriterOptions::suppressed()` with the default durability.
pub(crate) fn suppressed() -> WriterOptions {
    WriterOptions::suppressed().with_flush(port(DEFAULT_DURABILITY))
}
