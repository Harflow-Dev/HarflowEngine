// Other unix targets open these files without `O_NONBLOCK` and could block here.
#![cfg(any(target_os = "macos", target_os = "linux"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! A FIFO already in place of the fence or the due record is refused by `lstat` without
//! blocking. This does not exercise a FIFO swapped in between `lstat` and open.
//! This is the only test in its binary: creating a FIFO spawns `mkfifo`, and a child
//! spawned while sibling tests hold lease descriptors can briefly inherit them.
mod common;
mod read_session_support;
mod verify_support;
use read_session_support::seeded;
use std::{path::Path, sync::mpsc, time::Duration};
use verify_support::{FENCE, VERIFY, at, clear, is_incompatible, open_store};

fn make_fifo(path: &Path) {
    let status = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

/// Runs `check` on a helper thread and fails if it has not answered within 10 s.
fn bounded(check: impl FnOnce() -> bool + Send + 'static) {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(check());
    });
    assert!(receiver.recv_timeout(Duration::from_secs(10)).unwrap());
}

#[test]
fn a_fifo_fence_or_due_record_is_incompatible_without_blocking() {
    let seeded = seeded(3);
    let root = seeded.root.clone();
    make_fifo(&root.join(VERIFY));
    let store = open_store(&root).unwrap();
    bounded(
        move || matches!(store.verification_due(at(0)), Err(ref error) if is_incompatible(error)),
    );
    clear(&root, VERIFY);
    make_fifo(&root.join(FENCE));
    let opened = root.clone();
    bounded(move || matches!(open_store(&opened), Err(ref error) if is_incompatible(error)));
    clear(&root, FENCE);
    drop(seeded);
}
