#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_application::lease::LeaseMode;
use hf_domain::process::ProcessExit;
use hf_platform::{
    lease,
    process::{MacIdentity, OwnedSignal},
};
use std::{
    fs,
    process::{Command, Stdio},
    time::Instant,
};

#[path = "concurrent_spawn/admission.rs"]
mod admission;
#[path = "concurrent_spawn/support.rs"]
mod support;
#[path = "concurrent_spawn/workers.rs"]
mod workers;
use admission::CoordinatorAdmission;
use support::*;

#[test]
fn post_exec_descriptor_inventory_excludes_non_cloexec_parent_handles() {
    use nix::fcntl::{FcntlArg, FdFlag, fcntl};
    use std::{fs::OpenOptions, os::fd::AsFd};
    let temp = Temp::new();
    let inventory = temp.0.join("fds.txt");
    let file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(temp.0.join("sentinel"))
        .unwrap();
    fcntl(&file, FcntlArg::F_SETFD(FdFlag::empty())).unwrap();
    let (left, right) = nix::sys::socket::socketpair(
        nix::sys::socket::AddressFamily::Unix,
        nix::sys::socket::SockType::Stream,
        None,
        nix::sys::socket::SockFlag::empty(),
    )
    .unwrap();
    for fd in [left.as_fd(), right.as_fd()] {
        let _ = fcntl(fd, FcntlArg::F_GETFD).unwrap();
        fcntl(fd, FcntlArg::F_SETFD(FdFlag::empty())).unwrap();
    }
    assert_eq!(fcntl(&file, FcntlArg::F_GETFD).unwrap(), 0);
    let mut child = spawn(&["inventory", inventory.to_str().unwrap()]);
    wait_exit(&mut child);
    let values = read_small(&inventory);
    assert!(
        values.trim().is_empty(),
        "unexpected inherited descriptors: {values}"
    );
    drop(file);
    drop(left);
    drop(right);
}

#[test]
#[ignore = "isolated helper launched only by the stdio collision test"]
fn stdio_collision_helper() {
    let witness =
        std::env::var_os("HARFLOW_R04C_STDIO_WITNESS").expect("helper witness path missing");
    let inventory =
        std::env::var_os("HARFLOW_R04C_FD_INVENTORY").expect("helper fd inventory path missing");
    for fd in 0..=2 {
        nix::unistd::close(fd).unwrap();
    }
    let mut child = spawn(&["inventory", inventory.to_str().unwrap()]);
    wait_exit(&mut child);
    fs::write(witness, b"stdio normalized\n").unwrap();
    std::process::exit(0);
}

#[test]
fn library_spawn_normalizes_stdio_when_zero_through_two_are_closed() {
    let temp = Temp::new();
    let witness = temp.0.join("result");
    let inventory = temp.0.join("fds.txt");
    let exe = std::env::current_exe().unwrap();
    let mut helper = ExternalChild(
        Command::new(exe)
            .arg("--exact")
            .arg("stdio_collision_helper")
            .arg("--ignored")
            .arg("--nocapture")
            .env("HARFLOW_R04C_STDIO_WITNESS", &witness)
            .env("HARFLOW_R04C_FD_INVENTORY", &inventory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let end = deadline(3_000);
    loop {
        if let Some(status) = helper.0.try_wait().unwrap() {
            assert!(status.success(), "closed-stdio helper process failed");
            assert_eq!(read_small(&witness), "stdio normalized\n");
            assert!(read_small(&inventory).trim().is_empty());
            break;
        }
        assert!(
            Instant::now() < end,
            "closed-stdio helper exceeded its deadline"
        );
        std::hint::spin_loop();
    }
}

#[test]
fn shared_coordinator_excludes_external_record_lock_during_concurrent_spawns() {
    let _admission = CoordinatorAdmission::acquire();
    let temp = Temp::new();
    let sentinel = temp.0.join("root.lock");
    fs::write(&sentinel, []).unwrap();
    let coordinator = lease::shared();
    let root = coordinator
        .acquire(&sentinel, LeaseMode::Exclusive, false, deadline(1_000))
        .unwrap();
    assert!(
        external_lock_probe(&sentinel),
        "kernel witness acquired lock while root lease held"
    );
    let monitor = Probe::start(&sentinel);

    let mut children = workers::launch_concurrently();
    for child in &mut children {
        child.0.write_stdin(b"release\n", deadline(1_000)).unwrap();
        child.0.close_stdin();
        wait_exit(&mut child.0);
    }
    let (attempts, acquired) = monitor.finish();
    assert!(
        attempts > 0,
        "continuous independent probe made no attempts"
    );
    assert_eq!(
        acquired, 0,
        "external lock was acquired while root lease was held"
    );
    drop(root);
    assert!(
        !external_lock_probe(&sentinel),
        "kernel witness still sees lock after final lease drop"
    );
}

#[test]
fn exclusive_and_shared_coordinator_modes_match_kernel_lock_witness() {
    let _admission = CoordinatorAdmission::acquire();
    let temp = Temp::new();
    let sentinel = temp.0.join("mode.lock");
    fs::write(&sentinel, []).unwrap();
    let coordinator = lease::shared();
    let shared = coordinator
        .acquire(&sentinel, LeaseMode::Shared, false, deadline(1_000))
        .unwrap();
    assert!(external_lock_probe(&sentinel));
    let second = coordinator
        .acquire(&sentinel, LeaseMode::Shared, false, deadline(1_000))
        .unwrap();
    assert!(external_lock_probe(&sentinel));
    drop(shared);
    assert!(external_lock_probe(&sentinel));
    drop(second);
    assert!(!external_lock_probe(&sentinel));
}

#[test]
fn actual_external_reap_loses_wait_authority_without_inventing_exit() {
    let mut child = Child::new(spawn(&["hold"]));
    assert_eq!(
        child.0.initialization(),
        hf_platform::process::SpawnInitialization::Ready
    );
    let identity = child.0.registered_identity().unwrap().clone();
    child.0.write_stdin(b"release\n", deadline(1_000)).unwrap();
    child.0.close_stdin();
    let pid = nix::unistd::Pid::from_raw(child.0.pid() as i32);
    let end = deadline(2_000);
    loop {
        match nix::sys::wait::waitpid(pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG)).unwrap() {
            nix::sys::wait::WaitStatus::Exited(_, _)
            | nix::sys::wait::WaitStatus::Signaled(_, _, _) => {
                child.mark_external_settled();
                break;
            }
            _ if Instant::now() < end => std::hint::spin_loop(),
            other => panic!("bounded independent waitpid timed out: {other:?}"),
        }
    }
    assert_eq!(child.0.try_wait().unwrap(), ProcessExit::Unobserved);
    assert_eq!(child.0.try_wait().unwrap(), ProcessExit::Unobserved);
    assert_eq!(
        child
            .0
            .request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(1_000))
            .unwrap(),
        hf_platform::process::SignalOutcome::Refused(
            hf_domain::process::IdentityObservation::Unknown
        )
    );
    // The process is already gone; dropping closes only the retained pipe ends.
}
