#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
//! Real reads of this process's ancestry, freshly spawned children and root-owned ancestries
//! found on this machine. Every test first evaluates whether this host build is qualified; an
//! unqualified host must refuse explicitly. Chain identities come from the kernel record and
//! are compared with libproc-derived identities wherever libproc can read the process.

use hf_application::process::IdentityPort;
use hf_platform::process::MacIdentity;
use hf_platform::task_runtime::caller::{
    CallerObservation, CallerProcessRecord, CallerRecordLayout, current_caller,
    decode_caller_record, observe_caller,
};
use libproc::{bsd_info::BSDInfo, proc_pid::pidinfo};
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use sysctl::{Ctl, Sysctl};

const QUALIFIED: (&str, &str) = ("25.6.0", "25G83");
const RECORD_LEN: usize = 648;
const MAX_ROOT_CHAIN_ATTEMPTS: usize = 50;
const SEARCH_BUDGET: Duration = Duration::from_secs(20);

fn os_string(name: &str) -> Option<String> {
    Ctl::new(name).ok()?.value_string().ok()
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(30)
}

/// Returns false after asserting the explicit refusal when this host is not qualified.
fn qualified_or_refused() -> bool {
    let release = os_string("kern.osrelease");
    let version = os_string("kern.osversion");
    if release.as_deref() == Some(QUALIFIED.0) && version.as_deref() == Some(QUALIFIED.1) {
        return true;
    }
    eprintln!("unqualified host ({release:?}, {version:?}): asserting refusal");
    assert!(current_caller(deadline()).is_err());
    false
}

enum Kernel {
    Gone,
    Record(CallerProcessRecord),
    Error(String),
}

/// `Gone` only for ESRCH, an exact zero-length read or an observed zombie (status 5); every
/// other failure is `Error`.
fn kernel_read(pid: u32) -> Kernel {
    let Ok(native) = i32::try_from(pid) else {
        return Kernel::Error(format!("pid {pid} out of range"));
    };
    let mib = [
        nix::libc::CTL_KERN,
        nix::libc::KERN_PROC,
        nix::libc::KERN_PROC_PID,
        native,
    ];
    let mut buf = [0u8; 1024];
    match hf_sys::read_into(&mib, &mut buf) {
        Err(error) if error.raw_os_error() == Some(nix::libc::ESRCH) => Kernel::Gone,
        Err(error) => Kernel::Error(format!("pid {pid} read: {error}")),
        Ok(0) => Kernel::Gone,
        // A zombie (SZOMB = 5) is an exited process the decoder refuses; treat it as gone.
        Ok(RECORD_LEN) if buf[36] == 5 => Kernel::Gone,
        Ok(RECORD_LEN) => {
            let layout = CallerRecordLayout::MacOsArm64Sdk26_5;
            match decode_caller_record(layout, pid, &buf[..RECORD_LEN]) {
                Ok(record) => Kernel::Record(record),
                Err(error) => Kernel::Error(format!("pid {pid} decode: {error}")),
            }
        }
        Ok(length) => Kernel::Error(format!("pid {pid} unexpected length {length}")),
    }
}

/// Why a kernel walk stopped early.
enum WalkStop {
    Gone,
    Other,
}

/// Kernel records from `pid` up to, excluding, init; an early stop when a link is gone (ESRCH,
/// zero length or a zombie), a read fails, the walk is too long or the optional absolute
/// deadline passes (checked around each read).
fn kernel_walk(mut pid: u32, until: Option<Instant>) -> Result<Vec<CallerProcessRecord>, WalkStop> {
    let expired = || until.is_some_and(|limit| Instant::now() >= limit);
    let mut records = Vec::new();
    while pid > 1 {
        if records.len() >= 128 || expired() {
            return Err(WalkStop::Other);
        }
        let record = match kernel_read(pid) {
            Kernel::Record(record) => record,
            Kernel::Gone => return Err(WalkStop::Gone),
            Kernel::Error(_) => return Err(WalkStop::Other),
        };
        if expired() {
            return Err(WalkStop::Other);
        }
        pid = record.parent;
        records.push(record);
    }
    Ok(records)
}

fn has_root_link(records: &[CallerProcessRecord]) -> bool {
    records.iter().any(|record| record.uid == 0)
}

fn root_link_in(observation: &CallerObservation) -> bool {
    observation
        .chain
        .iter()
        .any(|link| link.uid == 0 && link.identity.pid() > 1)
}

struct Sleeper(Child);
impl Sleeper {
    fn spawn() -> Self {
        let child = Command::new("/bin/sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sleep");
        Self(child)
    }
}
impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `(pid, uid)` rows from `/bin/ps` by absolute path, cleared environment, bounded wait.
fn process_rows(until: Instant) -> Option<Vec<(u32, u32)>> {
    let mut child = Command::new("/bin/ps")
        .args(["-axo", "pid=,uid="])
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("ps starts");
    let mut stdout = child.stdout.take().expect("ps stdout");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        let _ = sender.send(text);
    });
    let text = receiver.recv_timeout(until.saturating_duration_since(Instant::now()));
    let _ = child.kill();
    let _ = child.wait();
    let rows = text.ok()?;
    Some(
        rows.lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
            })
            .collect(),
    )
}

fn compare_with_libproc(observation: &CallerObservation) {
    let euid = nix::unistd::geteuid().as_raw();
    for link in observation.chain.iter().filter(|link| link.uid == euid) {
        let pid = link.identity.pid();
        let native = i32::try_from(pid).unwrap();
        let info = pidinfo::<BSDInfo>(native, 0).expect("libproc read of a user-owned link");
        assert_eq!(
            MacIdentity.identity(pid).as_ref(),
            Some(&link.identity),
            "identity of {pid}"
        );
        assert_eq!(info.pbi_ppid, link.parent, "parent of {pid}");
        assert_eq!(info.pbi_uid, link.uid, "uid of {pid}");
    }
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
fn kernel_identities_match_libproc_for_ancestry_and_spawned_children() {
    if !qualified_or_refused() {
        return;
    }
    compare_with_libproc(&current_caller(deadline()).expect("own ancestry"));
    let children = [Sleeper::spawn(), Sleeper::spawn()];
    for child in &children {
        let pid = child.0.id();
        let observation = observe_caller(pid, deadline()).expect("child ancestry");
        assert_eq!(observation.chain[0].identity.pid(), pid);
        assert_eq!(observation.chain[1].identity.pid(), std::process::id());
        compare_with_libproc(&observation);
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

enum Walk {
    Unchanged,
    Changed,
    Exhausted,
    Error(String),
}

/// Re-reads every link of an earlier walk: any gone link, or a changed start time, parent or
/// uid, is `Changed`; a read failure is `Error`.
fn rewalk(original: &[CallerProcessRecord], until: Instant) -> Walk {
    for before in original {
        if Instant::now() >= until {
            return Walk::Exhausted;
        }
        let read = kernel_read(before.pid);
        if Instant::now() >= until {
            return Walk::Exhausted;
        }
        match read {
            Kernel::Gone => return Walk::Changed,
            Kernel::Error(message) => return Walk::Error(message),
            Kernel::Record(after) => {
                if (
                    after.start_seconds,
                    after.start_microseconds,
                    after.parent,
                    after.uid,
                ) != (
                    before.start_seconds,
                    before.start_microseconds,
                    before.parent,
                    before.uid,
                ) {
                    return Walk::Changed;
                }
            }
        }
    }
    Walk::Unchanged
}

/// Searches same-uid processes whose kernel ancestry has a root-owned non-init link, under one
/// absolute deadline. The attempt cap counts only root-link candidates; every same-uid row is
/// scanned within the budget.
fn observe_other_root_chain() -> Option<CallerObservation> {
    let search_deadline = Instant::now() + SEARCH_BUDGET;
    let exhausted = |message: &str| eprintln!("D10b: search budget exhausted ({message})");
    let euid = nix::unistd::geteuid().as_raw();
    let Some(rows) = process_rows(search_deadline) else {
        exhausted("process enumeration");
        return None;
    };
    let mut attempts = 0;
    let mut skipped = 0;
    let mut gone = 0;
    for (pid, uid) in rows {
        if uid != euid || pid <= 1 || pid == std::process::id() {
            continue;
        }
        if Instant::now() >= search_deadline || attempts >= MAX_ROOT_CHAIN_ATTEMPTS {
            exhausted("candidates");
            return None;
        }
        let walk = match kernel_walk(pid, Some(search_deadline)) {
            Ok(walk) => walk,
            Err(stop) => {
                if Instant::now() >= search_deadline {
                    exhausted("preflight walk");
                    return None;
                }
                match stop {
                    WalkStop::Gone => {
                        gone += 1;
                        eprintln!("D10b: candidate {pid} skipped (gone or zombie)");
                    }
                    WalkStop::Other => {
                        skipped += 1;
                        eprintln!(
                            "D10b: candidate {pid} skipped (kernel read error or walk bound)"
                        );
                    }
                }
                continue;
            }
        };
        if !has_root_link(&walk[1..]) {
            continue;
        }
        attempts += 1;
        let result = observe_caller(pid, search_deadline.min(deadline()));
        let observation = match result {
            Ok(observation) if root_link_in(&observation) => return Some(observation),
            other => other,
        };
        if Instant::now() >= search_deadline {
            exhausted("observation");
            return None;
        }
        match rewalk(&walk, search_deadline) {
            Walk::Exhausted => {
                exhausted("re-walk");
                return None;
            }
            Walk::Changed => eprintln!("D10b: candidate {pid} changed, trying the next"),
            Walk::Unchanged => panic!("unchanged candidate {pid} gave {:?}", observation.err()),
            Walk::Error(message) => panic!("candidate {pid} could not be re-checked: {message}"),
        }
    }
    if Instant::now() >= search_deadline {
        exhausted("end of scan");
        return None;
    }
    eprintln!(
        "D10b: no root-ancestor chain available ({gone} candidates gone or zombie, {skipped} skipped on read errors or walk bound)"
    );
    None
}

#[test]
fn observe_caller_admits_root_owned_ancestor_when_present() {
    if !qualified_or_refused() {
        return;
    }
    let own = kernel_walk(std::process::id(), None)
        .ok()
        .expect("own kernel ancestry");
    if has_root_link(&own[1..]) {
        eprintln!("D10b branch: own ancestry has a root-owned non-init link");
        let observation = current_caller(deadline()).expect("root-owned ancestor admitted");
        assert!(root_link_in(&observation));
        return;
    }
    if let Some(observation) = observe_other_root_chain() {
        eprintln!("D10b branch: same-uid process under a root-owned non-init ancestor");
        assert!(root_link_in(&observation));
        compare_with_libproc(&observation);
    }
}
