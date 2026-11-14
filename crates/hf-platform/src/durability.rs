//! Flush ports for the commit watermark, one per durability mode.
//!
//! `FullFlush` asks the device to persist its write cache: on macOS an explicit
//! `fcntl(fd, F_FULLFSYNC)`, elsewhere `sync_all`. `FsyncOnly` hands the data to the
//! device without that barrier: on macOS a plain `fsync(2)`, because std's `sync_all`
//! and `sync_data` both issue `F_FULLFSYNC` there; elsewhere `sync_all`, which is
//! `fsync`. Neither falls back to the other on failure: an error is returned as is.
//! In particular a `FullFlush` that the file system or device refuses (`ENOTSUP`,
//! `ENODEV` or any other error) is never retried as a plain `fsync`; the caller sees
//! the error and fails closed.
use hf_application::durability::FlushPort;
use std::{fs::File, sync::Arc};

/// How the commit watermark is made durable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
    FullFlush,
    FsyncOnly,
}

/// The mode production writers use: an explicit device-cache flush (measured at A7b).
pub const DEFAULT_DURABILITY: Durability = Durability::FullFlush;

/// The flush port for `mode`.
pub fn port(mode: Durability) -> Arc<dyn FlushPort> {
    match mode {
        Durability::FullFlush => Arc::new(FullFlush),
        Durability::FsyncOnly => Arc::new(FsyncOnly),
    }
}

/// `F_FULLFSYNC` on macOS, `sync_all` elsewhere. Every error, including an
/// unsupported file system, is returned unchanged, with no fallback to `fsync`.
#[derive(Clone, Copy, Debug)]
pub struct FullFlush;

/// `fsync(2)` on macOS, `sync_all` elsewhere.
#[derive(Clone, Copy, Debug)]
pub struct FsyncOnly;

#[cfg(target_os = "macos")]
impl FlushPort for FullFlush {
    fn flush(&self, file: &File) -> std::io::Result<()> {
        use nix::fcntl::{FcntlArg, fcntl};
        retry(|| fcntl(file, FcntlArg::F_FULLFSYNC).map(drop))
    }
}

#[cfg(target_os = "macos")]
impl FlushPort for FsyncOnly {
    fn flush(&self, file: &File) -> std::io::Result<()> {
        retry(|| nix::unistd::fsync(file))
    }
}

/// Repeats a call interrupted by a signal, as std does for its own `sync_all`.
#[cfg(target_os = "macos")]
fn retry(mut call: impl FnMut() -> nix::Result<()>) -> std::io::Result<()> {
    loop {
        match call() {
            Err(nix::errno::Errno::EINTR) => {}
            other => return other.map_err(std::io::Error::from),
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl FlushPort for FullFlush {
    fn flush(&self, file: &File) -> std::io::Result<()> {
        file.sync_all()
    }
}

#[cfg(not(target_os = "macos"))]
impl FlushPort for FsyncOnly {
    fn flush(&self, file: &File) -> std::io::Result<()> {
        file.sync_all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, path::PathBuf};

    /// A directory unique to this test run, removed on drop even when the test fails.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let name = format!("hf-durability-{}-{nanos}", std::process::id());
            let path = std::env::temp_dir().join(name);
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn both_modes_flush_a_written_file() {
        let scratch = Scratch::new();
        let mut file = File::create(scratch.0.join("flushed")).unwrap();
        file.write_all(b"watermark").unwrap();
        for mode in [Durability::FullFlush, Durability::FsyncOnly] {
            port(mode).flush(&file).unwrap();
        }
    }
}
