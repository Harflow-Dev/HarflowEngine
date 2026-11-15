#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! The maintenance session against the production lease: a foreign `flock` holder, a Rust
//! store handle in another process, and a creation race on an absent lock. Every child is
//! killed on drop and every wait is bounded.
use hf_application::lease::{LeaseError, LeaseGuard, LeaseMode, LeasePort};
use hf_domain::capture::FileIdentity;
use hf_platform::ctl::recovery_fs::MacRecoveryFs;
use hf_storage::{Error, Limits, MaintenanceSession, SessionRequest, Store};
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const BUDGET: Duration = Duration::from_millis(250);
const CHILD_WAIT: Duration = Duration::from_secs(30);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-maintenance-exclusion-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Tests take turns: a child spawned while another test sets up or holds a lock can look
/// like a foreign holder (the same window the fixture guard of the other host tests covers).
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A child process that is killed and reaped when dropped.
struct Guarded(Child);
impl Guarded {
    fn ready(mut child: Child) -> Self {
        let stdout = child.stdout.take().unwrap();
        let guard = Self(child);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let mut output = BufReader::new(stdout);
            while output.read_line(&mut line).unwrap_or(0) > 0 {
                if line.contains("READY") {
                    let _ = sender.send(());
                    return;
                }
                line.clear();
            }
        });
        assert!(
            receiver.recv_timeout(CHILD_WAIT).is_ok(),
            "the child never reported READY"
        );
        guard
    }
    fn stop(mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Drop for Guarded {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Setup only: the production lease retains a descriptor per lock path for the life of the
/// process, so a root whose locks the test then removes is prepared with this plain `flock`
/// port and the production lease first meets the root inside the session under test.
struct SetupLease;
struct Held(#[allow(dead_code)] fs::File);
impl LeaseGuard for Held {}
impl LeasePort for SetupLease {
    fn acquire(
        &self,
        path: &Path,
        mode: LeaseMode,
        create: bool,
        deadline: Instant,
    ) -> Result<Box<dyn LeaseGuard>, LeaseError> {
        if Instant::now() >= deadline {
            return Err(LeaseError::DeadlineExceeded);
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .open(path)
            .map_err(|_| LeaseError::Invalid)?;
        match mode {
            LeaseMode::Shared => file.try_lock_shared(),
            LeaseMode::Exclusive => file.try_lock(),
        }
        .map_err(|_| LeaseError::Busy)?;
        Ok(Box::new(Held(file)))
    }
}

fn store(fixture: &Fixture) -> PathBuf {
    let root = fixture.0.join("store");
    let deadline = Instant::now() + Duration::from_secs(5);
    Store::initialize_fixture(&root, Limits::default(), deadline, Arc::new(SetupLease)).unwrap();
    root
}

fn id(path: &Path) -> FileIdentity {
    let metadata = fs::symlink_metadata(path).unwrap();
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn acquire(root: &Path) -> hf_storage::Result<MaintenanceSession> {
    let request = SessionRequest {
        root_identity: id(root),
        lock_identity: None,
    };
    MaintenanceSession::acquire_within(
        root,
        &request,
        Arc::new(MacRecoveryFs),
        hf_platform::lease::shared(),
        BUDGET,
    )
}

/// Reaches the same session once the foreign holder is gone; kernel locks of a killed
/// process are released at once, so a short bounded retry is enough.
fn acquire_soon(root: &Path) -> MaintenanceSession {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match acquire(root) {
            Ok(session) => return session,
            Err(Error::Busy) if Instant::now() < deadline => {}
            Err(error) => panic!("the lock never became free: {error:?}"),
        }
    }
}

/// A Python child that takes `flock(LOCK_SH)` on `path`, reports READY and then waits for
/// its stdin to close. An alarm ends it regardless, so it can never outlive the test.
fn python_holder(path: &Path) -> Guarded {
    let script = "import fcntl, signal, sys\n\
                  signal.alarm(60)\n\
                  handle = open(sys.argv[1], 'r+')\n\
                  fcntl.flock(handle, fcntl.LOCK_SH)\n\
                  print('READY', flush=True)\n\
                  sys.stdin.read()\n";
    let child = Command::new("python3")
        .args(["-I", "-c", script])
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    Guarded::ready(child)
}

#[test]
fn python_flock_shared_blocks_maintenance_cross_process() {
    let _turn = serial();
    let fixture = Fixture::new();
    let root = store(&fixture);
    let legacy = root.join(".harflow-store.lock");
    fs::write(&legacy, b"").unwrap();
    let holder = python_holder(&legacy);
    let started = Instant::now();
    assert!(matches!(acquire(&root), Err(Error::Busy)));
    // The attempt gave up after its budget, not at once and not unbounded.
    assert!(started.elapsed() >= BUDGET / 2);
    assert!(started.elapsed() < Duration::from_secs(10));
    holder.stop();
    drop(acquire_soon(&root));
}

#[test]
fn absent_lock_concurrent_creation_one_proceeds_other_is_active_owner() {
    let _turn = serial();
    for round in 0..10 {
        let fixture = Fixture::new();
        let root = store(&fixture);
        fs::remove_file(root.join("store.lock")).unwrap();
        fs::remove_file(root.join("io.lock")).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let (root, barrier) = (root.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    acquire(&root)
                })
            })
            .collect();
        // Both results are held before either session is dropped.
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let proceeded = results.iter().filter(|r| r.is_ok()).count();
        let refused = results
            .iter()
            .filter(|r| matches!(r, Err(Error::Busy)))
            .count();
        assert_eq!(
            (proceeded, refused),
            (1, 1),
            "round {round}: {:?}",
            results
                .iter()
                .map(|r| r.as_ref().map(|_| ()))
                .collect::<Vec<_>>()
        );
        assert!(root.join("store.lock").is_file());
    }
}

#[test]
#[ignore = "child process entry point"]
fn store_handle_helper() {
    let Ok(root) = std::env::var("HF_ME_ROOT") else {
        return;
    };
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(60));
        std::process::exit(2);
    });
    let _store = Store::open(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(5),
        hf_platform::lease::shared(),
    )
    .unwrap();
    println!("READY");
    std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
}

#[test]
fn rust_store_handle_in_child_process_is_active_owner() {
    let _turn = serial();
    let fixture = Fixture::new();
    let root = store(&fixture);
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store_handle_helper",
            "--ignored",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env("HF_ME_ROOT", &root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let holder = Guarded::ready(child);
    assert!(matches!(acquire(&root), Err(Error::Busy)));
    holder.stop();
    drop(acquire_soon(&root));
}

/// Child entry point: waits for the start file, then acquires the maintenance session and
/// reports READY (holding it until stdin closes), BUSY, or the error.
#[test]
#[ignore = "child process entry point"]
fn maintenance_helper() {
    let (Ok(root), Ok(go)) = (std::env::var("HF_ME_ROOT"), std::env::var("HF_ME_GO")) else {
        return;
    };
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(60));
        std::process::exit(2);
    });
    while !Path::new(&go).exists() {
        std::thread::sleep(Duration::from_millis(1));
    }
    match acquire(Path::new(&root)) {
        Ok(_session) => {
            println!("READY");
            std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
        }
        Err(Error::Busy) => println!("BUSY"),
        Err(error) => println!("ERR {error:?}"),
    }
}

#[test]
fn absent_lock_creation_race_against_a_child_process_has_one_winner() {
    let _turn = serial();
    for round in 0..5 {
        let fixture = Fixture::new();
        let root = store(&fixture);
        fs::remove_file(root.join("store.lock")).unwrap();
        fs::remove_file(root.join("io.lock")).unwrap();
        let go = fixture.0.join("go");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "maintenance_helper",
                "--ignored",
                "--nocapture",
                "--test-threads",
                "1",
            ])
            .env("HF_ME_ROOT", &root)
            .env("HF_ME_GO", &go)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let _guard = Guarded(child);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let mut output = BufReader::new(stdout);
            while output.read_line(&mut line).unwrap_or(0) > 0 {
                // libtest prints "test maintenance_helper ... " before the helper's own output.
                let reply = line.trim().rsplit("... ").next().unwrap_or_default();
                if reply.starts_with("READY")
                    || reply.starts_with("BUSY")
                    || reply.starts_with("ERR")
                {
                    let _ = sender.send(reply.to_owned());
                    return;
                }
                line.clear();
            }
        });
        fs::write(&go, b"").unwrap();
        let mine = acquire(&root);
        let theirs = receiver
            .recv_timeout(CHILD_WAIT)
            .expect("the child never reported");
        let mine_won = mine.is_ok();
        assert!(
            mine_won != (theirs == "READY"),
            "round {round}: parent {:?}, child {theirs}",
            mine.as_ref().map(|_| ())
        );
        if mine_won {
            assert_eq!(theirs, "BUSY", "round {round}");
        } else {
            assert!(matches!(mine, Err(Error::Busy)), "round {round}");
        }
    }
}
