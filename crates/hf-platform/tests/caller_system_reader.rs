#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
//! Real reads of this process's ancestry and PID 1. Every test first evaluates whether this
//! host build is qualified and whether a root-owned non-init ancestor is unreadable through
//! libproc (a Terminal or SSH session under `login`/`sshd`); either condition must make
//! `current_caller` refuse explicitly. The direct kernel-record comparison covers self and
//! parent only.

use hf_platform::task_runtime::caller::{CallerRecordLayout, current_caller, decode_caller_record};
use libproc::{bsd_info::BSDInfo, proc_pid::pidinfo};
use std::time::{Duration, Instant};
use sysctl::{Ctl, Sysctl};

const QUALIFIED: (&str, &str) = ("25.6.0", "25G83");
const RECORD_LEN: usize = 648;

fn os_string(name: &str) -> Option<String> {
    Ctl::new(name).ok()?.value_string().ok()
}

/// Returns false after asserting the explicit refusal when this host is not qualified, or when
/// a root-owned non-init link of this process's ancestry is refused by libproc.
fn qualified_or_refused() -> bool {
    let release = os_string("kern.osrelease");
    let version = os_string("kern.osversion");
    if release.as_deref() != Some(QUALIFIED.0) || version.as_deref() != Some(QUALIFIED.1) {
        eprintln!("unqualified host ({release:?}, {version:?}): asserting refusal");
        assert!(current_caller(Instant::now() + Duration::from_secs(30)).is_err());
        return false;
    }
    let mut pid = i32::try_from(std::process::id()).unwrap();
    for _ in 0..128 {
        if pid <= 1 {
            break;
        }
        let record = kernel_record(pid);
        if record.uid == 0 && pidinfo::<BSDInfo>(pid, 0).is_err() {
            eprintln!("root-owned ancestor {pid} is refused by libproc: asserting refusal");
            assert!(current_caller(Instant::now() + Duration::from_secs(30)).is_err());
            return false;
        }
        pid = i32::try_from(record.parent).unwrap();
    }
    assert!(pid <= 1, "ancestry exceeds 128 links");
    true
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(30)
}

fn kernel_record(pid: i32) -> hf_platform::task_runtime::caller::CallerProcessRecord {
    let mib = [
        nix::libc::CTL_KERN,
        nix::libc::KERN_PROC,
        nix::libc::KERN_PROC_PID,
        pid,
    ];
    let mut buf = [0u8; 1024];
    let length = hf_sys::read_into(&mib, &mut buf).expect("kern.proc.pid read");
    assert_eq!(length, RECORD_LEN);
    let pid = u32::try_from(pid).unwrap();
    decode_caller_record(
        CallerRecordLayout::MacOsArm64Sdk26_5,
        pid,
        &buf[..RECORD_LEN],
    )
    .expect("decoded record")
}

#[test]
fn observe_caller_uses_system_reader_end_to_end() {
    if !qualified_or_refused() {
        return;
    }
    let observation = current_caller(deadline()).expect("ordinary caller completes");
    assert_eq!(observation.init.uid, 0);
    assert!(!observation.chain.is_empty());
}

#[test]
fn live_ancestry_of_test_process_completes_to_init() {
    if !qualified_or_refused() {
        return;
    }
    let observation = current_caller(deadline()).expect("ordinary caller completes");
    let chain = &observation.chain;
    assert_eq!(chain[0].identity.pid(), std::process::id());
    let parent = std::os::unix::process::parent_id();
    if parent == 1 {
        assert_eq!(chain.len(), 1);
    } else {
        assert_eq!(chain[1].identity.pid(), parent);
    }
    assert_eq!(chain.last().unwrap().parent, 1);
    let boot = observation.init.boot_uuid.to_ascii_lowercase();
    assert!(chain.iter().all(|link| link.identity.boot_uuid() == boot));
}

#[test]
fn system_reader_self_and_parent_match_libproc_identity() {
    if !qualified_or_refused() {
        return;
    }
    let mut pids = vec![i32::try_from(std::process::id()).unwrap()];
    let parent = i32::try_from(std::os::unix::process::parent_id()).unwrap();
    if parent != 1 {
        pids.push(parent);
    }
    for pid in pids {
        let record = kernel_record(pid);
        let info = pidinfo::<BSDInfo>(pid, 0).expect("libproc read");
        assert_eq!(record.pid, info.pbi_pid, "pid {pid}");
        assert_eq!(record.parent, info.pbi_ppid, "parent of {pid}");
        assert_eq!(record.uid, info.pbi_uid, "uid of {pid}");
        assert_eq!(
            record.start_seconds, info.pbi_start_tvsec,
            "seconds of {pid}"
        );
        assert_eq!(
            record.start_microseconds, info.pbi_start_tvusec,
            "microseconds of {pid}"
        );
    }
}

#[test]
fn live_ancestry_with_expired_deadline_refuses() {
    if !qualified_or_refused() {
        return;
    }
    let expired = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    assert!(current_caller(expired).is_err());
}
