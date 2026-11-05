#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use hf_domain::{
    fixture::{Gate, Stage},
    ownership::Id,
    process::ProcessIdentity,
};
use hf_platform::fixture::{
    FixtureGate, append_effect, gate_packet, qualify_ledger, read_frame, write_all,
};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use std::{
    fs::{self, File, OpenOptions},
    net::Shutdown,
    os::{
        fd::AsFd,
        unix::{fs::symlink, net::UnixListener},
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-b2-platform-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn root(&self) -> PathBuf {
        let root = self.0.join("root");
        fs::create_dir(&root).unwrap();
        root
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn ledger(root: &Path) -> PathBuf {
    let path = root.parent().unwrap().join("r03-b2.effects");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    path
}
fn id(byte: char) -> Id {
    Id::new(byte.to_string().repeat(32)).unwrap()
}
fn identity() -> ProcessIdentity {
    ProcessIdentity::new(42, 100, 17, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}
fn await_gate_peer(listener: UnixListener, expected: Vec<u8>, deadline: Instant) -> bool {
    let mut fds = [PollFd::new(listener.as_fd(), PollFlags::POLLIN)];
    let left = deadline
        .checked_duration_since(Instant::now())
        .unwrap_or_default();
    let timeout = PollTimeout::try_from(left).unwrap();
    if poll(&mut fds, timeout).unwrap_or(0) != 1 {
        return false;
    }
    let Ok((stream, _)) = listener.accept() else {
        return false;
    };
    let Ok(received) = read_frame(&stream, deadline) else {
        return false;
    };
    received == expected
        && write_all(&stream, &expected, deadline).is_ok()
        && stream.shutdown(Shutdown::Write).is_ok()
}

#[test]
fn fixture_gate_exchanges_bounded_bound_packet_and_rejects_non_socket_endpoints() {
    let scratch = Scratch::new();
    let path = scratch.path().join("gate.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let operation = id('1');
    let nonce = id('2');
    let stage = Stage::OwnerAfterClaim;
    let packet = gate_packet(stage, &operation, &nonce);
    let deadline = Instant::now() + Duration::from_secs(1);
    let peer = thread::spawn(move || await_gate_peer(listener, packet, deadline));
    FixtureGate::new(Some(Gate {
        stage,
        path: path.to_string_lossy().into_owned(),
    }))
    .reach(stage, &operation, &nonce, deadline)
    .unwrap();
    assert!(peer.join().unwrap());

    let regular = scratch.path().join("regular");
    File::create(&regular).unwrap();
    assert!(
        FixtureGate::new(Some(Gate {
            stage,
            path: regular.to_string_lossy().into_owned()
        }))
        .reach(
            stage,
            &operation,
            &nonce,
            Instant::now() + Duration::from_secs(1)
        )
        .is_err()
    );
    let link = scratch.path().join("gate-link.sock");
    symlink(&path, &link).unwrap();
    assert!(
        FixtureGate::new(Some(Gate {
            stage,
            path: link.to_string_lossy().into_owned()
        }))
        .reach(
            stage,
            &operation,
            &nonce,
            Instant::now() + Duration::from_secs(1)
        )
        .is_err()
    );
}

#[test]
fn effect_ledger_is_fixed_exclusive_and_rejects_identity_changes_and_links() {
    let scratch = Scratch::new();
    let root = scratch.root();
    let path = ledger(&root);
    let original = File::open(&path).unwrap();
    let qualified = qualify_ledger(&root).unwrap();
    let operation = id('3');
    let child = identity();
    append_effect(
        &root,
        qualified,
        &operation,
        &child,
        Instant::now() + Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        format!(
            "{}|{}|{}|{}|{}\n",
            operation.as_str(),
            child.pid(),
            child.start_seconds(),
            child.start_microseconds(),
            child.boot_uuid()
        )
    );
    drop(original);
    fs::remove_file(&path).unwrap();
    File::create(&path).unwrap();
    assert!(
        append_effect(
            &root,
            qualified,
            &operation,
            &child,
            Instant::now() + Duration::from_secs(1)
        )
        .is_err()
    );

    let linked_parent = scratch.path().join("linked");
    fs::create_dir(&linked_parent).unwrap();
    let linked = linked_parent.join("root");
    fs::create_dir(&linked).unwrap();
    let link_path = linked_parent.join("r03-b2.effects");
    symlink(&path, &link_path).unwrap();
    assert!(qualify_ledger(&linked).is_err());
}

#[test]
fn effect_ledger_lock_contention_refuses_without_waiting_or_appending() {
    let scratch = Scratch::new();
    let root = scratch.root();
    let path = ledger(&root);
    let qualified = qualify_ledger(&root).unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    lock.lock().unwrap();
    let result = append_effect(
        &root,
        qualified,
        &id('4'),
        &identity(),
        Instant::now() + Duration::from_secs(1),
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&path).unwrap(), Vec::<u8>::new());
}
