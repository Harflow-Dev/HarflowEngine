use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Read,
        os::unix::fs::symlink,
        process::{Child, Command, ExitStatus, Stdio},
        sync::atomic::{AtomicUsize, Ordering},
        sync::{Mutex, mpsc},
    };

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn case_guard() -> std::sync::MutexGuard<'static, ()> {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            match TEST_LOCK.try_lock() {
                Ok(guard) => return guard,
                Err(std::sync::TryLockError::Poisoned(error)) => return error.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) if Instant::now() < until => {
                    std::thread::yield_now();
                }
                Err(std::sync::TryLockError::WouldBlock) => panic!("test case admission deadline"),
            }
        }
    }

    fn root() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "hf-lease-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::canonicalize(root).unwrap()
    }

    fn acquire(path: &Path, mode: LeaseMode) -> Result<Box<dyn LeaseGuard>, LeaseError> {
        super::acquire(path, mode, true, Instant::now() + Duration::from_secs(1))
    }

    #[test]
    fn local_shared_exclusive_and_last_reader_follow_one_kernel_lock() {
        let _serial = case_guard();
        let root = root();
        let path = root.join("lease");
        let first = acquire(&path, LeaseMode::Shared).unwrap();
        let second = acquire(&path, LeaseMode::Shared).unwrap();
        assert_eq!(
            acquire(&path, LeaseMode::Exclusive).err(),
            Some(LeaseError::Busy)
        );
        drop(first);
        assert_eq!(
            acquire(&path, LeaseMode::Exclusive).err(),
            Some(LeaseError::Busy)
        );
        drop(second);
        let exclusive = acquire(&path, LeaseMode::Exclusive).unwrap();
        drop(exclusive);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shared_holder_limit_is_1024_and_restores_after_all_holders_drop() {
        let _serial = case_guard();
        let root = root();
        let path = root.join("lease");
        let mut holders = Vec::with_capacity(1024);
        for _ in 0..1024 {
            holders.push(acquire(&path, LeaseMode::Shared).unwrap());
        }
        assert_eq!(
            acquire(&path, LeaseMode::Shared).err(),
            Some(LeaseError::LimitExceeded)
        );
        drop(holders.pop());
        holders.push(acquire(&path, LeaseMode::Shared).unwrap());
        drop(holders);
        let exclusive = acquire(&path, LeaseMode::Exclusive).unwrap();
        drop(exclusive);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn independent_roots_progress_and_expired_admission_refuses() {
        let _serial = case_guard();
        let one = root();
        let two = root();
        let held = acquire(&one.join("lease"), LeaseMode::Exclusive).unwrap();
        let independent = acquire(&two.join("lease"), LeaseMode::Exclusive).unwrap();
        let expired = super::acquire(
            &one.join("other"),
            LeaseMode::Exclusive,
            true,
            Instant::now(),
        );
        assert_eq!(expired.err(), Some(LeaseError::DeadlineExceeded));
        drop((held, independent));
        let _ = fs::remove_dir_all(one);
        let _ = fs::remove_dir_all(two);
    }

    #[test]
    fn symlink_replacement_and_parent_path_fail_closed() {
        let _serial = case_guard();
        let root = root();
        let path = root.join("lease");
        let held = acquire(&path, LeaseMode::Shared).unwrap();
        let alias = root.join("alias");
        symlink(&path, &alias).unwrap();
        assert_eq!(
            acquire(&alias, LeaseMode::Shared).err(),
            Some(LeaseError::Invalid)
        );

        fs::rename(&path, root.join("old-lease")).unwrap();
        fs::write(&path, b"replacement").unwrap();
        assert_eq!(
            acquire(&path, LeaseMode::Shared).err(),
            Some(LeaseError::Replaced)
        );
        assert_eq!(
            super::acquire(
                &root.join("missing/lease"),
                LeaseMode::Shared,
                true,
                Instant::now() + Duration::from_secs(1)
            )
            .err(),
            Some(LeaseError::Invalid)
        );
        drop(held);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_candidate_descriptors_are_quarantined_and_count_toward_limit() {
        let _serial = case_guard();
        let root = root();
        let target = root.join("target");
        fs::write(&target, b"sentinel").unwrap();
        let parent_meta = fs::metadata(&root).unwrap();
        let parent = (parent_meta.dev(), parent_meta.ino());
        let mut local = State::default();
        for index in 0..256 {
            let alias = root.join(format!("alias-{index}"));
            fs::hard_link(&target, &alias).unwrap();
            assert_eq!(
                open(
                    &mut local,
                    &alias,
                    parent,
                    false,
                    Instant::now() + Duration::from_secs(1)
                )
                .err(),
                Some(LeaseError::Invalid)
            );
        }
        assert_eq!(local.quarantine.len(), 256);
        assert_eq!(
            open(
                &mut local,
                &target,
                parent,
                false,
                Instant::now() + Duration::from_secs(1)
            )
            .err(),
            Some(LeaseError::LimitExceeded)
        );
        // The quarantine intentionally retains descriptors until process exit.
        std::mem::forget(local);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn modeled_raced_alias_candidate_stays_quarantined_and_registered_lock_survives() {
        let _serial = case_guard();
        let root = root();
        let registered = root.join("registered");
        let held = acquire(&registered, LeaseMode::Shared).unwrap();
        let alias = root.join("alias");
        fs::hard_link(&registered, &alias).unwrap();

        // This directly exercises `open` after an acquisition precheck race. It
        // models the duplicate-open path; it is not an OS race reproduction.
        let parent_metadata = fs::metadata(&root).unwrap();
        let parent = (parent_metadata.dev(), parent_metadata.ino());
        let mut raced = State::default();
        assert_eq!(
            open(
                &mut raced,
                &alias,
                parent,
                false,
                Instant::now() + Duration::from_secs(1)
            )
            .err(),
            Some(LeaseError::Invalid)
        );
        assert_eq!(raced.quarantine.len(), 1);
        let candidate = raced.quarantine[0].metadata().unwrap();
        assert_eq!(candidate.ino(), fs::metadata(&registered).unwrap().ino());

        assert_eq!(external_lock_probe(&alias), "BLOCKED");
        drop(held);
        assert_eq!(external_lock_probe(&alias), "ACQUIRED");
        std::mem::forget(raced);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn external_lock_probe_helper() {
        let Ok(path) = std::env::var("HARFLOW_R04C_LEASE_PROBE") else {
            return;
        };
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        let result = match set(&file, nix::libc::F_WRLCK) {
            Ok(()) => {
                set(&file, nix::libc::F_UNLCK).unwrap();
                "ACQUIRED"
            }
            Err(LeaseError::Busy) => "BLOCKED",
            Err(error) => panic!("external lock probe failed: {error:?}"),
        };
        println!("R04C_LOCK={result}");
    }

    struct ProbeChild(Child);
    impl Drop for ProbeChild {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                reap_or_abort(&mut self.0);
            }
        }
    }

    fn reap_or_abort(child: &mut Child) {
        let end = Instant::now() + Duration::from_secs(2);
        loop {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            if Instant::now() >= end {
                std::process::abort();
            }
            std::thread::yield_now();
        }
    }

    fn wait_child(child: &mut Child) -> Result<ExitStatus, String> {
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) if Instant::now() < end => std::thread::yield_now(),
                Ok(None) => {
                    let _ = child.kill();
                    reap_or_abort(child);
                    return Err("external lock probe timed out".into());
                }
                Err(error) => {
                    let _ = child.kill();
                    reap_or_abort(child);
                    return Err(error.to_string());
                }
            }
        }
    }

    fn join_reader(reader: std::thread::JoinHandle<()>) {
        let end = Instant::now() + Duration::from_secs(2);
        while !reader.is_finished() && Instant::now() < end {
            std::thread::yield_now();
        }
        if !reader.is_finished() {
            std::process::abort();
        }
        let _ = reader.join();
    }

    fn external_lock_probe(path: &Path) -> String {
        let mut child = ProbeChild(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "lease::darwin::lease_tests::tests::external_lock_probe_helper",
                    "--nocapture",
                ])
                .env("HARFLOW_R04C_LEASE_PROBE", path)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let stdout = child.0.stdout.take().unwrap();
        let (send, receive) = mpsc::sync_channel(1);
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stdout.take(257).read_to_end(&mut bytes).map(|_| bytes);
            let _ = send.send(result);
        });
        let status = wait_child(&mut child.0);
        let bytes = receive
            .recv_timeout(Duration::from_secs(1))
            .map_err(|_| "external lock output timeout".to_owned())
            .and_then(|result| result.map_err(|error| error.to_string()));
        join_reader(reader);
        let status = status.expect("external lock probe wait failed");
        assert!(status.success(), "external lock probe failed: {status:?}");
        let bytes = bytes.expect("external lock output read failed");
        assert!(
            bytes.len() <= 256,
            "external lock output exceeded its bound"
        );
        let output = String::from_utf8(bytes).unwrap();
        output
            .lines()
            .find_map(|line| line.strip_prefix("R04C_LOCK="))
            .expect("external lock probe omitted result")
            .to_owned()
    }
}
