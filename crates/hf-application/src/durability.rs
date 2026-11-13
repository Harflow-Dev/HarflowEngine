//! Port for making a state-root file durable. The storage writer calls it where the
//! protocol needs a flush that SQLite does not provide itself.
use std::path::Path;

pub trait FlushPort: Send + Sync + std::fmt::Debug {
    /// Makes the contents of the regular file at `path` durable before returning.
    fn flush(&self, path: &Path) -> std::io::Result<()>;
}
