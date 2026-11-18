//! Real reads of root-owned non-init processes through the production record path.
use super::*;
use libproc::{bsd_info::BSDInfo, proc_pid::pidinfo};
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

const MAX_CANDIDATES: usize = 20;

/// `(pid, uid, command)` rows from `/bin/ps` by absolute path, cleared environment and a
/// bounded wait.
fn process_rows() -> Vec<(u32, u32, String)> {
    let mut child = Command::new("/bin/ps")
        .args(["-axo", "pid=,uid=,comm="])
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
    let text = receiver.recv_timeout(Duration::from_secs(10));
    let _ = child.kill();
    let _ = child.wait();
    text.expect("ps output in time")
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let uid = fields.next()?.parse().ok()?;
            Some((pid, uid, fields.collect::<Vec<_>>().join(" ")))
        })
        .collect()
}

fn candidates() -> Vec<(u32, String)> {
    let mut roots: Vec<_> = process_rows()
        .into_iter()
        .filter(|(pid, uid, _)| *uid == 0 && *pid > 1)
        .map(|(pid, _, command)| (pid, command))
        .collect();
    let rank = |command: &str| match command.rsplit('/').next() {
        Some("logd") => 0,
        Some("sandboxd") => 1,
        _ => 2,
    };
    roots.sort_by_key(|(pid, command)| (rank(command), *pid));
    roots.truncate(MAX_CANDIDATES);
    roots
}

fn same_incarnation(a: &CallerProcessRecord, b: &CallerProcessRecord) -> bool {
    a.pid == b.pid
        && a.uid == 0
        && b.uid == 0
        && a.start_seconds == b.start_seconds
        && a.start_microseconds == b.start_microseconds
}

#[test]
fn kernel_reader_reads_root_owned_non_init_process() {
    let (release, version) = SystemKernel.os_strings().expect("os strings");
    if qualify(std::env::consts::ARCH, &release, &version).is_none() {
        eprintln!("unqualified host ({release}, {version}): asserting refusal");
        assert!(read_record(1).is_err());
        return;
    }
    let unprivileged = nix::unistd::getuid().as_raw() != 0;
    let mut stable = Vec::new();
    for (pid, command) in candidates() {
        let Ok(first) = read_record(pid) else {
            eprintln!("candidate {pid} {command}: skipped (kernel read refused or exited)");
            continue;
        };
        let libproc = pidinfo::<BSDInfo>(i32::try_from(pid).unwrap(), 0);
        match read_record(pid) {
            Ok(second) if same_incarnation(&first, &second) => {
                eprintln!(
                    "candidate {pid} {command}: kernel read stable, parent {}, libproc {}",
                    first.parent,
                    if libproc.is_ok() {
                        "readable"
                    } else {
                        "refused"
                    }
                );
                if unprivileged && libproc.is_ok() {
                    eprintln!("note: libproc read a root-owned process for this caller");
                }
                assert!(first.parent >= 1 && first.parent <= i32::MAX as u32);
                assert!(
                    read_record(first.parent).is_ok(),
                    "parent {} unreadable",
                    first.parent
                );
                stable.push(command);
            }
            _ => eprintln!("candidate {pid} {command}: skipped (changed between reads)"),
        }
    }
    assert!(
        !stable.is_empty(),
        "no root-owned process was stably readable"
    );
    eprintln!("root-owned processes read: {stable:?}");
}
