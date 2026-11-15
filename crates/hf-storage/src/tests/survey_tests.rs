#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! In-crate coverage of the survey: prerequisite short-circuiting in both modes, and the
//! maintenance mode's view of a store with no `-wal` and a fence.
use crate::{
    Error, Limits, Store,
    binding::Binding,
    maintenance::{MaintenanceSession, SessionRequest},
    survey::{Check, Live, Mode, Outcome, Steps, run},
};
use hf_application::ctl::state_recover::RecoveryFsPort;
use hf_domain::capture::FileIdentity;
use std::{
    cell::RefCell,
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct TestFs;
impl RecoveryFsPort for TestFs {
    fn create_lock(&self, root: &Path, name: &str) -> std::io::Result<bool> {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(root.join(name))
        {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(error),
        }
    }
    fn free_space(&self, _: &Path) -> std::io::Result<u64> {
        Ok(u64::MAX)
    }
}

/// Every step the survey makes, in order, delegating to the real checks.
#[derive(Default)]
struct Counting(RefCell<Vec<Check>>);
impl Counting {
    fn seen(&self, check: Check) {
        self.0.borrow_mut().push(check);
    }
}
impl Steps for Counting {
    fn sidecars(&self, b: &Binding) -> crate::Result<()> {
        self.seen(Check::Sidecars);
        Live.sidecars(b)
    }
    fn marker(&self, b: &Binding) -> crate::Result<crate::Identity> {
        self.seen(Check::Marker);
        Live.marker(b)
    }
    fn header(&self, b: &Binding) -> crate::Result<()> {
        self.seen(Check::Header);
        Live.header(b)
    }
    fn journal(&self, b: &Binding) -> crate::Result<()> {
        self.seen(Check::Journal);
        Live.journal(b)
    }
    fn wal(&self, b: &Binding) -> crate::Result<()> {
        self.seen(Check::Wal);
        Live.wal(b)
    }
    fn acceptance(&self, root: &Path, generation: &str) -> crate::Result<()> {
        self.seen(Check::Acceptance);
        Live.acceptance(root, generation)
    }
    fn fence(&self, root: &Path, generation: &str) -> crate::Result<()> {
        self.seen(Check::Fence);
        Live.fence(root, generation)
    }
    fn watermark(&self, root: &Path, generation: &str) -> crate::Result<u64> {
        self.seen(Check::Watermark);
        Live.watermark(root, generation)
    }
    fn binding(&self, b: &Binding) -> crate::Result<()> {
        self.seen(Check::Binding);
        Live.binding(b)
    }
}

fn store(name: &str) -> (Scratch, PathBuf, String) {
    let parent =
        std::env::temp_dir().join(format!("hf-storage-survey-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let scratch = Scratch(fs::canonicalize(parent).unwrap());
    let root = scratch.0.join("store");
    let leases = crate::tests::lease::lease_port();
    let deadline = Instant::now() + Duration::from_secs(5);
    let opened = Store::initialize_fixture(&root, Limits::default(), deadline, leases).unwrap();
    let generation = opened.identity().generation.clone();
    drop(opened);
    (scratch, root, generation)
}

fn id(path: &Path) -> FileIdentity {
    let metadata = fs::symlink_metadata(path).unwrap();
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn session(root: &Path) -> MaintenanceSession {
    let request = SessionRequest {
        root_identity: id(root),
        lock_identity: Some(id(&root.join("store.lock"))),
    };
    let leases = crate::tests::lease::lease_port();
    MaintenanceSession::acquire(root, &request, Arc::new(TestFs), leases).unwrap()
}

fn binding(root: &Path) -> Binding {
    Binding::identify_existing(root, crate::tests::lease::lease_port()).unwrap()
}

fn fifo(path: &Path) {
    let _ = fs::remove_file(path);
    assert!(Command::new("mkfifo").arg(path).status().unwrap().success());
}

/// A refusal makes later files unsafe to open: with FIFOs at the fence and the watermark
/// (an open would hang the test) the survey stops where it refused, and the counting seam
/// confirms that no later step ran.
#[test]
fn precheck_mode_never_opens_later_files_after_a_refusal() {
    type Damage = fn(&Path);
    let cases: [(&str, Damage, Vec<Check>); 3] = [
        (
            "non-regular store.lock",
            |root| {
                fs::remove_file(root.join("store.lock")).unwrap();
                fs::create_dir(root.join("store.lock")).unwrap();
            },
            vec![Check::Sidecars],
        ),
        (
            "replaced marker",
            |root| {
                let bytes = fs::read(root.join("store.identity")).unwrap();
                fs::remove_file(root.join("store.identity")).unwrap();
                fs::write(root.join("store.identity"), bytes).unwrap();
            },
            vec![Check::Sidecars],
        ),
        (
            "bad header",
            |root| {
                let mut bytes = fs::read(root.join("store.sqlite3")).unwrap();
                bytes[0] = b'X';
                fs::write(root.join("store.sqlite3"), bytes).unwrap();
            },
            vec![Check::Sidecars, Check::Marker, Check::Header],
        ),
    ];
    for (name, damage, expected) in cases {
        let (_scratch, root, _) = store(&format!("precheck-stops-{}", name.len()));
        let bound = binding(&root);
        fifo(&root.join("store.fence"));
        fifo(&root.join("store.watermark"));
        damage(&root);
        let counting = Counting::default();
        let survey = run(&bound, Mode::Precheck, &counting);
        assert_eq!(*counting.0.borrow(), expected, "{name}");
        for check in [Check::Fence, Check::Watermark, Check::Binding] {
            assert!(
                matches!(survey.outcome(check), Outcome::NotPerformed),
                "{name}"
            );
        }
        assert!(survey.require_clean().is_err(), "{name}");
    }
}

#[test]
fn maintenance_mode_skips_all_after_unsafe_sidecar() {
    let (_scratch, root, _) = store("maintenance-sidecar");
    let held = session(&root);
    let _ = fs::remove_file(root.join("store.sqlite3-shm"));
    fs::create_dir(root.join("store.sqlite3-shm")).unwrap();
    let bound = binding(&root);
    let counting = Counting::default();
    let survey = run(&bound, Mode::Maintenance(&held.authority), &counting);
    assert_eq!(*counting.0.borrow(), vec![Check::Sidecars]);
    assert!(matches!(
        survey.outcome(Check::Fence),
        Outcome::NotPerformed
    ));
}

#[test]
fn maintenance_mode_reads_fence_and_watermark_with_wal_missing() {
    let (_scratch, root, generation) = store("maintenance-fence");
    let held = session(&root);
    fs::remove_file(root.join("store.sqlite3-wal")).unwrap();
    let mut fence = fs::File::create(root.join("store.fence")).unwrap();
    write!(fence, "harflow-fence-1\n{generation}\ncommit_lost\n").unwrap();
    let survey = held.survey().unwrap();
    assert!(matches!(
        survey.outcome(Check::Wal),
        Outcome::Failed(Error::WalMissing)
    ));
    assert!(matches!(
        survey.outcome(Check::Fence),
        Outcome::Failed(Error::CommitLost)
    ));
    assert!(matches!(survey.outcome(Check::Watermark), Outcome::Passed));
    assert!(matches!(survey.outcome(Check::Binding), Outcome::Passed));
    assert!(survey.mark().is_some());
    // The precheck mode gives today's first error for the same store.
    let precheck = run(&binding(&root), Mode::Precheck, &Live);
    assert!(matches!(precheck.require_clean(), Err(Error::WalMissing)));
}
