#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The `ps` based process view and the pid observer against the real process table.
//!
//! The helper path, timeout and output cap are private constants of the implementation, so
//! the timeout kill and the 4 MiB cap have no seam here.
use hf_application::ctl::task_reads::ProcessViewPort;
use hf_domain::ctl::caller::PidObservation;
use hf_platform::process::PsProcessView;
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(120)
}

#[test]
fn the_view_names_this_process_its_session_and_its_parent_first() {
    let view = PsProcessView::system().view(deadline()).unwrap();
    assert_eq!(view.pid, std::process::id());
    assert!(!view.ancestors.contains(&view.pid));
    assert_eq!(
        view.ancestors.first().copied(),
        Some(std::os::unix::process::parent_id())
    );
    assert!(view.ancestors.iter().all(|pid| *pid > 1));
    let mut seen = view.ancestors.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), view.ancestors.len(), "no cycles");
    assert!(view.session != 0);
}

#[test]
fn a_live_child_is_present_a_reaped_one_is_absent_and_a_zombie_is_not_present() {
    let mut child = Command::new("/bin/sleep")
        .arg("30")
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let PidObservation::Present(identity) = PsProcessView::system().observe(pid) else {
        panic!("a live child must be present");
    };
    assert_eq!(identity.pid(), pid);
    assert_eq!(
        PsProcessView::system().observe(pid),
        PsProcessView::system().observe(pid),
        "stable identity"
    );
    child.kill().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    // Until it is reaped the child is a zombie, which is never reported as present.
    let mut settled = false;
    while Instant::now() < deadline {
        match PsProcessView::system().observe(pid) {
            PidObservation::Present(_) => std::thread::sleep(Duration::from_millis(5)),
            other => {
                assert!(!matches!(other, PidObservation::Present(_)), "{other:?}");
                settled = true;
                break;
            }
        }
    }
    assert!(
        settled,
        "the killed child was still reported present at the deadline"
    );
    child.wait().unwrap();
    assert_eq!(PsProcessView::system().observe(pid), PidObservation::Absent);
}

#[test]
fn pids_zero_one_and_out_of_range_are_unknown() {
    for pid in [0, 1, u32::MAX] {
        assert_eq!(
            PsProcessView::system().observe(pid),
            PidObservation::Unknown,
            "{pid}"
        );
    }
}

#[test]
fn another_users_process_is_never_absent() {
    let out = Command::new("/bin/ps")
        .args(["-axo", "pid=,user="])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let me = Command::new("/usr/bin/id").arg("-un").output().unwrap();
    let me = String::from_utf8_lossy(&me.stdout).trim().to_string();
    assert_ne!(
        me, "root",
        "this test needs a non-root user to observe a root-owned process"
    );
    let root_pid = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid: u32 = fields.next()?.parse().ok()?;
            (fields.next()? == "root" && pid > 1).then_some(pid)
        })
        .next()
        .expect("no root-owned process is visible, so the cross-user case cannot be exercised");
    let seen = PsProcessView::system().observe(root_pid);
    assert!(
        !matches!(seen, PidObservation::Absent),
        "a root-owned pid that exists must not be reported absent: {seen:?}"
    );
}

mod helper {
    //! The process view against fake helper executables written to a temporary directory.
    //! A real setuid `ps` kill is not exercised: a fake script is not setuid, so the privilege
    //! path of the kill has no coverage here.
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, sync::Mutex};

    // Writing and then running scripts from parallel threads races on a busy text file, so
    // every helper test holds this lock from the write to the end.
    static WRITE: Mutex<()> = Mutex::new(());

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn dir(name: &str) -> Dir {
        let base = std::env::temp_dir().join(format!("hf-ps-{name}-{}", std::process::id()));
        fs::create_dir_all(&base).unwrap();
        Dir(fs::canonicalize(&base).unwrap())
    }

    fn script(dir: &Dir, body: &str) -> PathBuf {
        let path = dir.0.join("helper");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        WRITE.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn own() -> u32 {
        std::process::id()
    }

    fn quick(program: PathBuf) -> PsProcessView {
        PsProcessView::with_helper(program, Duration::from_secs(20))
    }

    #[test]
    fn a_well_formed_table_gives_the_chain() {
        let _serial = serial();
        let d = dir("good");
        let program = script(
            &d,
            &format!("echo '{} 4242'\necho '4242 1'\necho '1 0'", own()),
        );
        let view = quick(program).view(deadline()).unwrap();
        assert_eq!(view.ancestors, vec![4242]);
        assert_eq!(view.pid, own());
    }

    #[test]
    fn malformed_output_nonzero_exit_and_a_missing_pid_are_a_failure_to_look() {
        let _serial = serial();
        let me = own();
        let cases = [
            ("garbage", "echo 'abc def'".to_string()),
            ("extra field", format!("echo '{me} 4242 9'")),
            ("negative", format!("echo '{me} -4'")),
            ("one field", format!("echo '{me}'")),
            ("empty", String::new()),
            ("nonzero", format!("echo '{me} 4242'\nexit 3")),
            ("missing pid", "echo '5 1'\necho '6 5'".to_string()),
            ("not utf8", format!("printf '{me} 4242\\n\\377\\n'")),
        ];
        for (name, body) in cases {
            let d = dir("bad");
            let program = script(&d, &body);
            assert!(quick(program).view(deadline()).is_err(), "{name}");
        }
        let missing = PsProcessView::with_helper("/nonexistent/helper", Duration::from_secs(5));
        assert!(missing.view(deadline()).is_err());
    }

    /// A helper whose whole output is exactly `total` bytes, every line a valid pair.
    fn table_of(total: usize) -> String {
        let me = own();
        let first = format!("{me} 4242");
        let pad = (4 - (first.len() + 1) % 4) % 4;
        let head = first.len() + pad + 1;
        format!(
            "printf '%s%*s\\n' '{first}' {pad} ''\nyes '1 1' | head -c {}",
            total - head
        )
    }

    #[test]
    fn output_is_capped_at_exactly_four_mebibytes() {
        let _serial = serial();
        let cap = 4 * 1024 * 1024;
        for (total, ok) in [
            (cap - 4, true),
            (cap, true),
            (cap + 4, false),
            (cap + 400_000, false),
        ] {
            let d = dir("cap");
            let program = script(&d, &table_of(total));
            assert_eq!(quick(program).view(deadline()).is_ok(), ok, "{total} bytes");
        }
    }

    fn pid_file(path: &std::path::Path) -> u32 {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            if let Ok(text) = fs::read_to_string(path)
                && let Ok(pid) = text.trim().parse()
            {
                return pid;
            }
            assert!(
                Instant::now() < until,
                "the helper never wrote {}",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_slow_helper_times_out_within_the_budget_and_is_killed_and_reaped() {
        let _serial = serial();
        let d = dir("slow");
        let marker = d.0.join("pid");
        let program = script(
            &d,
            &format!("echo $$ > '{}'\nexec /bin/sleep 600", marker.display()),
        );
        let view = PsProcessView::with_helper(program, Duration::from_millis(3000));
        let started = Instant::now();
        assert!(view.view(deadline()).is_err());
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );
        let pid = pid_file(&marker);
        // Killed and reaped: a zombie would still be reported, so only absence passes.
        assert_eq!(PsProcessView::system().observe(pid), PidObservation::Absent);
    }

    #[test]
    fn half_of_the_remaining_time_bounds_the_helper_so_the_snapshot_keeps_budget() {
        let _serial = serial();
        let d = dir("half");
        let program = script(&d, "exec /bin/sleep 600");
        let view = PsProcessView::with_helper(program, Duration::from_secs(60));
        let started = Instant::now();
        assert!(view.view(Instant::now() + Duration::from_secs(4)).is_err());
        let spent = started.elapsed();
        assert!(
            spent < Duration::from_millis(3500),
            "{spent:?}: more than half of 4 s"
        );
    }

    #[test]
    fn stdout_held_open_past_the_deadline_cannot_extend_it() {
        let _serial = serial();
        let d = dir("held");
        let holder = d.0.join("holder");
        let me = own();
        let program = script(
            &d,
            &format!(
                "echo '{me} 4242'\n/bin/sleep 600 &\necho $! > '{}'\nexit 0",
                holder.display()
            ),
        );
        let view = PsProcessView::with_helper(program, Duration::from_millis(3000));
        let started = Instant::now();
        let outcome = view.view(deadline());
        let spent = started.elapsed();
        let background = pid_file(&holder);
        // The leftover holder is not the helper; stop it so no process outlives the test.
        let _ = Command::new("/bin/kill")
            .arg(background.to_string())
            .status();
        assert!(
            outcome.is_err(),
            "output never closed, so it is not a complete table"
        );
        assert!(spent < Duration::from_secs(10), "{spent:?}");
    }
}
