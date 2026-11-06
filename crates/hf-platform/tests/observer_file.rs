#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_platform::qualification::live::ObserverFile;
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "harflow-observer-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn creates_private_empty_file_and_reads_parent_append() {
    let path = path("append");
    let observer = ObserverFile::create(&path).unwrap();
    let metadata = fs::symlink_metadata(&path).unwrap();
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    assert_eq!(metadata.dev(), observer.device());
    assert_eq!(metadata.ino(), observer.inode());
    assert!(observer.read_bounded().unwrap().is_empty());

    let mut append = fs::OpenOptions::new().append(true).open(&path).unwrap();
    append
        .write_all(b"CLIENT_PRELOAD_READY_V1\t0123456789abcdef0123456789abcdef\t7\n")
        .unwrap();
    drop(append);
    assert_eq!(
        observer.read_bounded().unwrap(),
        b"CLIENT_PRELOAD_READY_V1\t0123456789abcdef0123456789abcdef\t7\n"
    );
    drop(observer);
    fs::remove_file(path).unwrap();
}

#[test]
fn refuses_existing_path_and_symlink_without_opening_target() {
    let target = path("target");
    fs::write(&target, b"preserve").unwrap();
    let link = path("symlink");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(ObserverFile::create(&link).is_err());
    assert!(ObserverFile::create(&target).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"preserve");
    fs::remove_file(link).unwrap();
    fs::remove_file(target).unwrap();
}

#[test]
fn detects_replaced_path_while_retaining_original_descriptor() {
    let original = path("original");
    let replacement = path("replacement");
    let observer = ObserverFile::create(&original).unwrap();
    fs::rename(&original, &replacement).unwrap();
    fs::write(&original, b"replacement").unwrap();
    assert!(observer.read_bounded().is_err());
    fs::remove_file(original).unwrap();
    fs::remove_file(replacement).unwrap();
}

#[test]
fn refuses_hardlinks_and_content_over_the_bound() {
    let hardlinked = path("hardlink");
    let observer = ObserverFile::create(&hardlinked).unwrap();
    let alias = path("alias");
    fs::hard_link(&hardlinked, &alias).unwrap();
    assert!(observer.read_bounded().is_err());
    drop(observer);
    fs::remove_file(alias).unwrap();
    fs::remove_file(hardlinked).unwrap();

    let oversized = path("oversized");
    let observer = ObserverFile::create(&oversized).unwrap();
    let mut append = fs::OpenOptions::new()
        .append(true)
        .open(&oversized)
        .unwrap();
    append.write_all(&[b'x'; 513]).unwrap();
    drop(append);
    assert!(observer.read_bounded().is_err());
    drop(observer);
    fs::remove_file(oversized).unwrap();
}
