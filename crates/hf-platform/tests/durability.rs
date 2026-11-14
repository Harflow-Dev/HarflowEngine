#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The commit-watermark flush ports: both modes flush a regular file, read-only or
//! write-only, and both return the error of a descriptor that cannot be flushed rather
//! than falling back to the other mode.
use hf_application::durability::FlushPort;
use hf_platform::durability::{DEFAULT_DURABILITY, Durability, FsyncOnly, FullFlush, port};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hf-platform-durability-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const MODES: [Durability; 2] = [Durability::FullFlush, Durability::FsyncOnly];

#[test]
fn the_default_is_the_full_flush() {
    assert_eq!(DEFAULT_DURABILITY, Durability::FullFlush);
}

#[test]
fn both_modes_flush_written_regular_files_through_any_access_mode() {
    let scratch = Scratch::new();
    let path = scratch.0.join("store.watermark");
    let mut file = File::create(&path).unwrap();
    file.write_all(&[7u8; 8192]).unwrap();
    for mode in MODES {
        port(mode).flush(&file).unwrap();
    }
    FullFlush.flush(&file).unwrap();
    FsyncOnly.flush(&file).unwrap();
    let read_only = File::open(&path).unwrap();
    let write_only = OpenOptions::new().write(true).open(&path).unwrap();
    for mode in MODES {
        port(mode).flush(&read_only).unwrap();
        port(mode).flush(&write_only).unwrap();
    }
    assert_eq!(fs::read(&path).unwrap(), vec![7u8; 8192]);
}

#[test]
fn a_descriptor_that_cannot_be_flushed_is_an_error_in_both_modes() {
    let (reader, _writer) = std::io::pipe().unwrap();
    let pipe = File::from(std::os::fd::OwnedFd::from(reader));
    let (socket, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let socket = File::from(std::os::fd::OwnedFd::from(socket));
    for mode in MODES {
        assert!(port(mode).flush(&pipe).is_err(), "{mode:?} flushed a pipe");
        assert!(
            port(mode).flush(&socket).is_err(),
            "{mode:?} flushed a socket"
        );
    }
}

/// `/dev/null` accepts `fsync(2)` but not `F_FULLFSYNC`: the full flush, and the default
/// port, report that error instead of falling back to the plain one.
#[cfg(target_os = "macos")]
#[test]
fn a_full_flush_the_device_refuses_does_not_fall_back_to_fsync() {
    let null = OpenOptions::new().write(true).open("/dev/null").unwrap();
    FsyncOnly.flush(&null).unwrap();
    let refused = FullFlush.flush(&null).unwrap_err();
    assert!(refused.raw_os_error().is_some(), "{refused:?}");
    let refused = port(DEFAULT_DURABILITY).flush(&null).unwrap_err();
    assert!(refused.raw_os_error().is_some(), "{refused:?}");
}
