#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_platform::task_runtime::space::{MIN_FREE_BYTES, preflight};
use std::{path::Path, time::Instant};

#[test]
fn rejects_empty_relative_and_overlarge_path_inventories_before_filesystem_observation() {
    assert!(preflight(&[], Instant::now() + std::time::Duration::from_secs(1)).is_err());
    assert!(
        preflight(
            &[Path::new("relative")],
            Instant::now() + std::time::Duration::from_secs(1)
        )
        .is_err()
    );
    let paths = [
        Path::new("/"),
        Path::new("/"),
        Path::new("/"),
        Path::new("/"),
        Path::new("/"),
    ];
    assert!(preflight(&paths, Instant::now() + std::time::Duration::from_secs(1)).is_err());
}

#[test]
fn expired_observation_refuses_without_mutating_any_path_and_floor_is_256_mib() {
    assert_eq!(MIN_FREE_BYTES, 256 * 1024 * 1024);
    let path = Path::new("/tmp/harflow-preflight-must-not-create");
    assert!(matches!(
        preflight(&[path], Instant::now()),
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut
    ));
    assert!(!path.exists());
}
