//! Port for making a state-root file durable. The storage writer calls it where the
//! protocol needs a flush that SQLite does not provide itself: the commit watermark.
use std::fs::File;

pub trait FlushPort: Send + Sync + std::fmt::Debug {
    /// Makes the contents of the open regular file `file` durable before returning. The
    /// caller keeps the descriptor, so the flush applies to the inode it wrote, never to
    /// a file that replaced it under the same name.
    fn flush(&self, file: &File) -> std::io::Result<()>;
}
