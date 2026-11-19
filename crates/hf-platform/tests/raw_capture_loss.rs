#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use hf_domain::{
    ownership::Id, process::ProcessIdentity, task_lifecycle::LifecycleBinding, task_read::TaskId,
    task_runtime::CaptureLoss,
};
use hf_platform::task_runtime::raw::{
    LossFile, PrivateRootError, RawCapture, RawRootHold, cause, classify_loss, classify_loss_file,
    open_private_root, private_root_error,
};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

static LEASE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The process-global lease state refuses concurrent takers (`Busy`): cases that prepare a
/// capture run one at a time, as in the other capture test files.
fn case_guard() -> std::sync::MutexGuard<'static, ()> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        match LEASE_TEST_LOCK.try_lock() {
            Ok(guard) => return guard,
            Err(std::sync::TryLockError::Poisoned(error)) => return error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) if Instant::now() < until => {
                std::thread::yield_now();
            }
            Err(std::sync::TryLockError::WouldBlock) => panic!("test case admission deadline"),
        }
    }
}

struct Hold;
impl RawRootHold for Hold {
    fn validate(&self, _: &LifecycleBinding) -> std::io::Result<()> {
        Ok(())
    }
}
struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let base = fs::canonicalize(std::env::temp_dir()).unwrap();
        let root = base.join(format!("hf-raw-loss-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        Self(root)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn id(digit: char) -> Id {
    Id::new(digit.to_string().repeat(32)).unwrap()
}
fn binding() -> LifecycleBinding {
    LifecycleBinding {
        task: TaskId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into()).unwrap(),
        operation: id('1'),
        attempt: id('2'),
        generation: id('3'),
    }
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(20)
}
fn payload(root: &Path) -> PathBuf {
    let b = binding();
    root.join("private-payload")
        .join(b.generation.as_str())
        .join(b.task.as_str())
        .join(b.attempt.as_str())
}
fn capture(root: &Path) -> RawCapture {
    let prepared = RawCapture::prepare(root, Box::new(Hold), binding(), deadline()).unwrap();
    let child = ProcessIdentity::new(
        7200,
        1_768_000_010,
        7200,
        "01234567-89ab-cdef-0123-456789abcdef".into(),
    )
    .unwrap();
    prepared.bind(child)
}

#[test]
fn classify_loss_exact_contents() {
    assert_eq!(classify_loss(b""), LossFile::None);
    assert_eq!(
        classify_loss(b"capture-known-loss stall\n"),
        LossFile::Stall
    );
    assert_eq!(
        classify_loss(b"capture-known-loss disconnect\n"),
        LossFile::Disconnect
    );
    assert_eq!(
        classify_loss(b"capture-known-loss\n"),
        LossFile::Unspecified
    );
    for other in [
        &b"capture-known-loss stall"[..],
        b"capture-known-loss stall\nx",
        b"capture-known-loss  stall\n",
        b"\n",
        b"x",
    ] {
        assert_eq!(classify_loss(other), LossFile::UnknownCause, "{other:?}");
    }
}

#[test]
fn absent_loss_file_reads_unknown_cause() {
    let scratch = Scratch::new("absent");
    assert_eq!(
        classify_loss_file(&scratch.0.join("loss.raw")),
        LossFile::UnknownCause
    );
    let present = scratch.0.join("present");
    fs::write(&present, b"capture-known-loss disconnect\n").unwrap();
    assert_eq!(classify_loss_file(&present), LossFile::Disconnect);
}

#[test]
fn raw_view_validates_cause_lengths() {
    let _serial = case_guard();
    for (cause, line) in [
        (CaptureLoss::Stall, &b"capture-known-loss stall\n"[..]),
        (
            CaptureLoss::Disconnect,
            &b"capture-known-loss disconnect\n"[..],
        ),
    ] {
        let scratch = Scratch::new(&format!("lengths-{cause:?}"));
        let mut raw = capture(&scratch.0);
        let loss = payload(&scratch.0).join("loss.raw");
        assert_eq!(fs::metadata(&loss).unwrap().len(), 0);
        raw.record_known_loss(cause, deadline()).unwrap();
        assert_eq!(fs::read(&loss).unwrap(), line);
        raw.view(deadline()).unwrap();
        // The idempotent same cause rewrites nothing.
        raw.record_known_loss(cause, deadline()).unwrap();
        assert_eq!(fs::read(&loss).unwrap(), line);
        let tampered = std::fs::OpenOptions::new()
            .append(true)
            .open(&loss)
            .unwrap();
        std::io::Write::write_all(&mut &tampered, b"x").unwrap();
        assert!(
            raw.view(deadline()).is_err(),
            "length differs from the committed cause"
        );
    }
}

#[test]
fn equal_length_overwrite_of_the_loss_file_is_refused_and_poisons() {
    let _serial = case_guard();
    let scratch = Scratch::new("overwrite");
    let mut raw = capture(&scratch.0);
    raw.record_known_loss(CaptureLoss::Stall, deadline())
        .unwrap();
    let loss = payload(&scratch.0).join("loss.raw");
    let file = fs::OpenOptions::new().write(true).open(&loss).unwrap();
    std::os::unix::fs::FileExt::write_all_at(&file, b"capture-known-loss stalL\n", 0).unwrap();
    assert_eq!(fs::metadata(&loss).unwrap().len(), 25);
    assert!(
        raw.record_known_loss(CaptureLoss::Stall, deadline())
            .is_err()
    );
    assert!(raw.is_poisoned());
    assert!(raw.view(deadline()).is_err());
}

#[test]
fn second_inconsistent_loss_refused_and_poisons() {
    let _serial = case_guard();
    let scratch = Scratch::new("second");
    let mut raw = capture(&scratch.0);
    raw.record_known_loss(CaptureLoss::Stall, deadline())
        .unwrap();
    assert!(
        raw.record_known_loss(CaptureLoss::Disconnect, deadline())
            .is_err()
    );
    assert!(raw.is_poisoned());
    let loss = payload(&scratch.0).join("loss.raw");
    assert_eq!(fs::read(loss).unwrap(), b"capture-known-loss stall\n");
    let none_scratch = Scratch::new("none");
    let mut none = capture(&none_scratch.0);
    assert!(
        none.record_known_loss(CaptureLoss::None, deadline())
            .is_err()
    );
}

#[test]
fn private_root_rejects_group_bits_symlink_noncanonical_foreign_uid() {
    let scratch = Scratch::new("predicate");
    let root = &scratch.0;
    let opened = open_private_root(root).unwrap();
    assert_eq!(opened.identity().inode, fs::metadata(root).unwrap().ino());
    let loose = root.join("loose");
    fs::create_dir(&loose).unwrap();
    for mode in [0o750, 0o705, 0o755, 0o770] {
        fs::set_permissions(&loose, fs::Permissions::from_mode(mode)).unwrap();
        assert!(open_private_root(&loose).is_err(), "{mode:o}");
    }
    fs::set_permissions(&loose, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(open_private_root(&loose).is_ok());
    let link = root.join("link");
    symlink(&loose, &link).unwrap();
    assert!(open_private_root(&link).is_err(), "symlink");
    assert!(
        open_private_root(&root.join("loose/../loose")).is_err(),
        "not canonical"
    );
    assert!(open_private_root(Path::new("relative")).is_err());
    assert!(open_private_root(&root.join("missing")).is_err());
    let kind = |path: &Path| private_root_error(&open_private_root(path).err().unwrap());
    fs::set_permissions(&loose, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(kind(&loose), PrivateRootError::LooseMode);
    assert_eq!(kind(&link), PrivateRootError::NotCanonical);
    assert_eq!(
        kind(&root.join("loose/../loose")),
        PrivateRootError::NotCanonical
    );
    assert_eq!(kind(Path::new("relative")), PrivateRootError::NotCanonical);
    assert_eq!(kind(&root.join("missing")), PrivateRootError::Io);
    let plain = root.join("plain");
    fs::write(&plain, b"").unwrap();
    assert_eq!(kind(&plain), PrivateRootError::NotDirectory);
    assert_eq!(
        kind(Path::new("/private/var/empty")),
        PrivateRootError::ForeignOwner
    );
    // Owned by another user and open to group and other.
    assert!(
        open_private_root(Path::new("/private/var/empty")).is_err(),
        "foreign owner"
    );
}

fn record() -> Vec<u8> {
    b"harflow-owner-retained-cause v1\nphase: tick\nkind: io\nerrno: 5\nsource: -\nend\n".to_vec()
}

#[test]
fn retained_cause_is_written_once_with_mode_0600() {
    let scratch = Scratch::new("cause");
    let root = open_private_root(&scratch.0).unwrap();
    cause::write_retained_cause(&scratch.0, root.identity(), &binding(), &record()).unwrap();
    let file = payload(&scratch.0).join(cause::CAUSE_FILE);
    assert_eq!(fs::read(&file).unwrap(), record());
    let m = fs::metadata(&file).unwrap();
    assert_eq!(m.mode() & 0o777, 0o600);
    // Never replaced.
    let other = b"harflow-owner-retained-cause v1\nend\n".to_vec();
    assert!(cause::write_retained_cause(&scratch.0, root.identity(), &binding(), &other).is_err());
    assert_eq!(fs::read(&file).unwrap(), record());
    for dir in ["private-payload", "private-payload/".to_owned().as_str()] {
        assert_eq!(fs::metadata(scratch.0.join(dir)).unwrap().mode() & 0o077, 0);
    }
}

#[test]
fn cause_refuses_symlinked_component_and_loose_root_and_wrong_identity() {
    let scratch = Scratch::new("cause-refusals");
    let root = open_private_root(&scratch.0).unwrap();
    let elsewhere = scratch.0.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::set_permissions(&elsewhere, fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&elsewhere, scratch.0.join("private-payload")).unwrap();
    assert!(
        cause::write_retained_cause(&scratch.0, root.identity(), &binding(), &record()).is_err()
    );
    assert!(
        fs::read_dir(&elsewhere).unwrap().next().is_none(),
        "nothing written through the link"
    );
    fs::remove_file(scratch.0.join("private-payload")).unwrap();
    let other = open_private_root(&elsewhere).unwrap();
    assert!(
        cause::write_retained_cause(&scratch.0, other.identity(), &binding(), &record()).is_err(),
        "identity"
    );
    for bad in [&b"no terminator"[..], &vec![b'x'; 5000]] {
        assert!(cause::write_retained_cause(&scratch.0, root.identity(), &binding(), bad).is_err());
    }
    fs::set_permissions(&scratch.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        cause::write_retained_cause(&scratch.0, root.identity(), &binding(), &record()).is_err()
    );
    assert!(!scratch.0.join("private-payload").exists());
}

#[test]
fn cause_partial_write_lacks_terminator() {
    let scratch = Scratch::new("cause-short");
    let root = open_private_root(&scratch.0).unwrap();
    let short =
        cause::write_retained_cause_capped(&scratch.0, root.identity(), &binding(), &record(), 20);
    assert!(short.is_err());
    let file = payload(&scratch.0).join(cause::CAUSE_FILE);
    let bytes = fs::read(&file).unwrap();
    assert_eq!(bytes.len(), 20);
    assert!(!bytes.ends_with(b"end\n"));
    assert!(
        cause::write_retained_cause(&scratch.0, root.identity(), &binding(), &record()).is_err(),
        "no retry"
    );
    assert_eq!(fs::read(&file).unwrap(), bytes);
}
