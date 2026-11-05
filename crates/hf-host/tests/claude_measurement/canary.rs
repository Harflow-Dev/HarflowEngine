#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::fs;

pub(super) fn verify_and_remove(path: &std::path::Path, initial: &fs::Metadata) {
    let after = fs::symlink_metadata(path).unwrap();
    assert!(super::identity::same_metadata(initial, &after));
    assert_eq!(fs::read(path).unwrap(), b"outside-user-data");
    assert!(super::identity::same_metadata(
        initial,
        &fs::symlink_metadata(path).unwrap()
    ));
    fs::remove_file(path).expect("remove settled harness-created read canary");
}
