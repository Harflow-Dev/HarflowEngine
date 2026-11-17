#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Rechecking the originally prepared pin rejects a changed candidate before a spawn spec
//! can be returned. These tests use owned temporary files and never launch a child.
use hf_application::{
    prepared_launch::{StreamDigestPort, StreamingDigestFactory},
    recovery::RecoveryError,
};
use hf_domain::ownership::Digest;
use hf_platform::{backend_launch::prepare_version_probe, prepared_launch::observe_binary};
use std::{
    ffi::OsString,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("hf-backend-pin-{}-{suffix}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct DigestStream(u64);
impl StreamDigestPort for DigestStream {
    fn update(&mut self, bytes: &[u8]) -> Result<(), RecoveryError> {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
        Ok(())
    }
    fn finish(self) -> Result<Digest, RecoveryError> {
        Digest::new(format!("{:064x}", self.0)).map_err(|_| RecoveryError::Invalid)
    }
}
struct DigestFactory;
impl StreamingDigestFactory for DigestFactory {
    type Stream = DigestStream;
    fn start(&self, _: usize) -> Result<Self::Stream, RecoveryError> {
        Ok(DigestStream(0xcbf29ce484222325))
    }
}

fn executable(path: &Path, contents: &[u8]) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn changed_binary_bytes_and_logical_target_refuse_the_original_version_probe_pin() {
    let scratch = Scratch::new();
    let physical = scratch.0.join("claude-real");
    let logical = scratch.0.join("claude");
    let replacement = scratch.0.join("claude-replacement");
    executable(&physical, b"#!/bin/sh\nexit 0\n");
    executable(&replacement, b"#!/bin/sh\nexit 1\n");
    symlink(&physical, &logical).unwrap();

    let factory = DigestFactory;
    let mut expected = observe_binary(&physical, &factory, deadline()).unwrap();
    expected.logical_path = logical.to_str().unwrap().into();
    let environment = vec![(OsString::from("PATH"), scratch.0.as_os_str().to_owned())];

    executable(&physical, b"#!/bin/sh\nexit 2\n");
    assert!(matches!(
        prepare_version_probe(&expected, &environment, &factory, deadline()),
        Err(hf_domain::task_launch::LaunchRefusal::IdentityChanged)
    ));

    fs::remove_file(&logical).unwrap();
    symlink(&replacement, &logical).unwrap();
    assert!(matches!(
        prepare_version_probe(&expected, &environment, &factory, deadline()),
        Err(hf_domain::task_launch::LaunchRefusal::IdentityChanged)
    ));
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
