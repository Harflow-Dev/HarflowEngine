use super::{
    capture::ActualCapture,
    support::{Delivery, deadline},
};
use hf_application::prepared_launch::CaptureContext;
use std::{
    fs,
    fs::OpenOptions,
    io::Write,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

pub(super) fn assert_raw_mutations(actual: &ActualCapture, scratch: &Path, original: &[u8]) {
    let path = raw_path(actual, scratch);
    assert_rewrite_refused(actual, &path, original);
    assert_hardlink_refused(actual, scratch, &path, original);
    assert_symlink_refused(actual, scratch, &path);
    assert_replacement_refused(actual, scratch, &path, original);
    assert_extent_refusals(actual, &path, original);
}
fn raw_path(actual: &ActualCapture, scratch: &Path) -> PathBuf {
    scratch.join(format!(
        "r03-b2c.{}.stdout",
        actual.request.operation.as_str()
    ))
}
fn raw_refuses(actual: &ActualCapture) -> bool {
    actual
        .reader
        .read(
            CaptureContext {
                invitation: &actual.receipt.invitation,
                request: &actual.request,
                source: &actual.source,
            },
            &Delivery(Some(actual.receipt.clone())),
            deadline(3),
        )
        .is_err()
}
fn assert_rewrite_refused(actual: &ActualCapture, path: &Path, original: &[u8]) {
    let mut altered = original.to_vec();
    altered[0] = if altered[0] == b'{' { b' ' } else { b'{' };
    fs::write(path, altered).unwrap();
    assert!(raw_refuses(actual), "same-inode rewrite refused");
    fs::write(path, original).unwrap();
}
fn assert_hardlink_refused(actual: &ActualCapture, scratch: &Path, path: &Path, original: &[u8]) {
    let alias = scratch.join("stdout-hardlink");
    fs::hard_link(path, &alias).unwrap();
    assert!(raw_refuses(actual), "hard-linked capture refused");
    fs::remove_file(alias).unwrap();
    fs::write(path, original).unwrap();
}
fn assert_symlink_refused(actual: &ActualCapture, scratch: &Path, path: &Path) {
    let saved = scratch.join("stdout-original");
    fs::rename(path, &saved).unwrap();
    symlink(&saved, path).unwrap();
    assert!(raw_refuses(actual), "symlink capture refused");
    fs::remove_file(path).unwrap();
    fs::rename(saved, path).unwrap();
}
fn assert_replacement_refused(
    actual: &ActualCapture,
    scratch: &Path,
    path: &Path,
    original: &[u8],
) {
    let saved = scratch.join("stdout-original");
    fs::rename(path, &saved).unwrap();
    fs::write(path, original).unwrap();
    assert!(raw_refuses(actual), "replacement inode refused");
    fs::remove_file(path).unwrap();
    fs::rename(saved, path).unwrap();
}
fn assert_extent_refusals(actual: &ActualCapture, path: &Path, original: &[u8]) {
    fs::write(path, &original[..original.len() - 1]).unwrap();
    assert!(raw_refuses(actual), "truncated capture refused");
    fs::write(path, original).unwrap();
    OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(b"tail")
        .unwrap();
    assert!(raw_refuses(actual), "unaccepted raw tail refused");
    fs::write(path, original).unwrap();
}
