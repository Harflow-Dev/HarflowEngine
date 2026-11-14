#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! In-crate coverage of the state-recovery gate's post-failure re-read
//! (`recovery_gate::after_failure`), the listed small-file read the gate retries on, and
//! the `commit_lost` fence writer's generation check. Record and fence bytes are written
//! here directly.
use crate::{
    Error, Limits, Store,
    binding::Binding,
    fence, recovery_gate,
    small_file::{self, Content, Listed},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const RECORD: &str = "store.recovery";
const OP: &str = "0123456789abcdef0123456789abcdef";

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

/// An initialized store root under a fresh scratch directory.
fn store(name: &str) -> (Scratch, PathBuf) {
    let parent = std::env::temp_dir().join(format!(
        "hf-storage-recovery-gate-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let scratch = Scratch(fs::canonicalize(parent).unwrap());
    let root = scratch.0.join("store");
    let leases = crate::tests::lease::lease_port();
    drop(Store::initialize_fixture(&root, Limits::default(), deadline(), leases).unwrap());
    (scratch, root)
}

fn record(state: &str) -> String {
    format!("harflow-recovery-1\n{OP}\n{state}\n")
}

/// The production identification step of `open_existing`, after its lease-free read.
fn captured(root: &Path) -> crate::Result<Binding> {
    recovery_gate::capture_after_gate(root, crate::tests::lease::lease_port())
}

/// A listing source that answers `answers` in order and counts its calls.
fn scripted(
    answers: Vec<Listed>,
    calls: &std::cell::Cell<u32>,
) -> impl FnMut() -> crate::Result<Listed> + '_ {
    let mut answers = answers.into_iter();
    move || {
        calls.set(calls.get() + 1);
        Ok(answers.next().expect("no more scripted answers"))
    }
}

fn bytes(state: &str) -> Listed {
    Listed::Content(Content::Bytes(record(state).into_bytes()))
}

fn removed() -> Listed {
    Listed::Removed(std::io::Error::from(std::io::ErrorKind::NotFound))
}

#[test]
fn a_replaced_then_pending_record_is_recovery_interrupted() {
    let calls = std::cell::Cell::new(0);
    let got = recovery_gate::check_with(scripted(vec![Listed::Replaced, bytes("pending")], &calls));
    assert!(matches!(got, Err(Error::RecoveryInterrupted)), "{got:?}");
    assert_eq!(calls.get(), 2);
}

#[test]
fn a_removed_then_present_record_is_decoded() {
    let calls = std::cell::Cell::new(0);
    let got = recovery_gate::read_with(scripted(vec![removed(), bytes("committed")], &calls));
    let record = got.unwrap().unwrap();
    assert!(record.commits(OP));
    assert_eq!(calls.get(), 2);
}

#[test]
fn a_record_replaced_on_every_attempt_stops_at_the_bound_as_incompatible() {
    let attempts = recovery_gate::REPLACED_RETRIES + 1;
    let calls = std::cell::Cell::new(0);
    let answers = (0..attempts + 2).map(|_| Listed::Replaced).collect();
    let got = recovery_gate::read_with(scripted(answers, &calls));
    assert!(matches!(got, Err(Error::Incompatible)), "{got:?}");
    assert_eq!(calls.get(), attempts);
}

#[test]
fn a_record_removed_on_every_attempt_stops_at_the_bound_as_io() {
    let attempts = recovery_gate::REPLACED_RETRIES + 1;
    let calls = std::cell::Cell::new(0);
    let answers = (0..attempts + 2).map(|_| removed()).collect();
    let got = recovery_gate::read_with(scripted(answers, &calls));
    assert!(
        matches!(&got, Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound),
        "{got:?}"
    );
    assert_eq!(calls.get(), attempts);
}

/// Sets `stop` when dropped, so a panicking reader still ends the writer thread.
struct StopOnDrop<'a>(&'a AtomicBool);
impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Upper bound on the race writer's turns and time, whatever the readers do.
const WRITER_TURNS: u32 = 1_000_000;
const WRITER_TIME: Duration = Duration::from_secs(30);

fn label(result: crate::Result<Binding>) -> String {
    match result {
        Ok(_) => "captured".to_string(),
        Err(Error::Io(error)) => format!("Io({:?})", error.kind()),
        Err(error) => format!("{error:?}"),
    }
}

#[test]
fn after_failure_maps_a_moved_marker_or_database_to_recovery_interrupted_only_when_pending() {
    for (index, moved) in ["store.identity", "store.sqlite3"].into_iter().enumerate() {
        let (_scratch, root) = store(&format!("moved-{index}"));
        fs::rename(root.join(moved), root.with_file_name(moved)).unwrap();
        let original = label(Binding::capture(
            &root,
            true,
            crate::tests::lease::lease_port(),
        ));
        assert!(
            original == "Incompatible" || original.starts_with("Io("),
            "{moved}: {original}"
        );
        assert_eq!(label(captured(&root)), original, "{moved}: no record");
        fs::write(root.join(RECORD), record("committed")).unwrap();
        assert_eq!(label(captured(&root)), original, "{moved}: committed");
        fs::write(root.join(RECORD), record("pending")).unwrap();
        assert!(
            matches!(captured(&root), Err(Error::RecoveryInterrupted)),
            "{moved}: pending"
        );
        assert!(
            matches!(
                recovery_gate::open_existing(&root, crate::tests::lease::lease_port()),
                Err(Error::RecoveryInterrupted)
            ),
            "{moved}: open_existing"
        );
    }
}

#[test]
fn after_failure_keeps_errors_it_does_not_map() {
    let (_scratch, root) = store("unmapped");
    fs::write(root.join(RECORD), record("pending")).unwrap();
    for error in [
        Error::Busy,
        Error::DeadlineExceeded,
        Error::Fenced,
        Error::CommitLost,
    ] {
        let want = format!("{error:?}");
        let got = recovery_gate::after_failure(&root, error);
        assert_eq!(format!("{got:?}"), want);
    }
    for error in [Error::InvalidRoot, Error::Incompatible] {
        let got = recovery_gate::after_failure(&root, error);
        assert!(matches!(got, Error::RecoveryInterrupted), "{got:?}");
    }
}

#[test]
fn after_failure_reports_an_unreadable_record_of_an_existing_root() {
    let (_scratch, root) = store("unreadable");
    let path = root.join(RECORD);
    fs::write(&path, record("pending")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    let binds = fs::read(&path).is_err();
    if binds {
        let got = recovery_gate::after_failure(&root, Error::Incompatible);
        assert!(
            matches!(&got, Error::Io(error) if error.kind() == std::io::ErrorKind::PermissionDenied),
            "{got:?}"
        );
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn a_stable_malformed_record_is_incompatible() {
    let (_scratch, root) = store("malformed");
    for bytes in [
        b"".to_vec(),
        b"\x00garbage".to_vec(),
        record("done").into_bytes(),
        [record("pending").into_bytes(), vec![b'x'; 128]].concat(),
    ] {
        fs::write(root.join(RECORD), &bytes).unwrap();
        let read = recovery_gate::read(&root);
        assert!(
            matches!(read, Err(Error::Incompatible)),
            "{bytes:?}: {read:?}"
        );
    }
    fs::write(root.join(RECORD), record("committed")).unwrap();
    let committed = recovery_gate::read(&root).unwrap().unwrap();
    assert!(!committed.is_pending() && committed.commits(OP));
}

#[test]
fn read_listed_classifies_what_it_can_see_without_a_race() {
    let (_scratch, root) = store("listed");
    let listed = |name: &str| small_file::read_listed(&root, name, 128);
    assert!(matches!(
        listed(RECORD),
        Ok(Listed::Content(Content::Absent))
    ));
    fs::write(root.join(RECORD), record("pending")).unwrap();
    assert!(
        matches!(listed(RECORD), Ok(Listed::Content(Content::Bytes(b))) if b == record("pending").as_bytes())
    );
    fs::write(root.join(RECORD), vec![b'x'; 129]).unwrap();
    assert!(matches!(
        listed(RECORD),
        Ok(Listed::Content(Content::Oversized))
    ));
    fs::remove_file(root.join(RECORD)).unwrap();
    fs::create_dir(root.join(RECORD)).unwrap();
    assert!(matches!(listed(RECORD), Err(Error::Incompatible)));
    fs::remove_dir(root.join(RECORD)).unwrap();
    std::os::unix::fs::symlink(root.join("store.identity"), root.join(RECORD)).unwrap();
    assert!(matches!(listed(RECORD), Err(Error::Incompatible)));
    let missing = small_file::read_listed(&root.join("no-such-dir"), RECORD, 128);
    assert!(matches!(missing, Ok(Listed::Content(Content::Absent))));
    let not_a_directory = small_file::read_listed(&root.join("store.identity"), RECORD, 128);
    assert!(
        matches!(not_a_directory, Err(Error::Io(_))),
        "{not_a_directory:?}"
    );
}

/// `Replaced` and `Removed` only arise between `lstat` and the open, which has no seam: a
/// bounded race replaces (rename over) or removes and recreates the file while reads run.
/// Every answer must be one of the two contents, `Replaced` or `Removed`, never an error.
#[test]
fn read_listed_reports_a_racing_replacement_or_removal_without_an_error() {
    let (_scratch, root) = store("listed-race");
    let pending = record("pending");
    let committed = record("committed");
    fs::write(root.join(RECORD), &committed).unwrap();
    let (mut replaced, mut removed) = (0, 0);
    for remove in [false, true] {
        let stop = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let _stop = StopOnDrop(&stop);
            scope.spawn(|| {
                let temporary = root.join("store.recovery.race-tmp");
                let until = Instant::now() + WRITER_TIME;
                for turn in 0..WRITER_TURNS {
                    if stop.load(Ordering::SeqCst) || Instant::now() >= until {
                        break;
                    }
                    let text = if turn % 2 == 0 { &pending } else { &committed };
                    if remove {
                        let _ = fs::remove_file(root.join(RECORD));
                        fs::write(root.join(RECORD), text).unwrap();
                    } else {
                        fs::write(&temporary, text).unwrap();
                        fs::rename(&temporary, root.join(RECORD)).unwrap();
                    }
                }
            });
            for _ in 0..4000 {
                match small_file::read_listed(&root, RECORD, 128) {
                    Ok(Listed::Replaced) => replaced += 1,
                    Ok(Listed::Removed(error)) => {
                        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
                        removed += 1;
                    }
                    Ok(Listed::Content(Content::Absent)) if remove => {}
                    Ok(Listed::Content(Content::Bytes(bytes))) => {
                        // A remove-and-recreate can show a file not yet written.
                        let whole = bytes == pending.as_bytes() || bytes == committed.as_bytes();
                        assert!(whole || (remove && bytes.is_empty()), "{bytes:?}");
                    }
                    other => panic!("remove {remove}: {other:?}"),
                }
            }
            stop.store(true, Ordering::SeqCst);
        });
    }
    eprintln!("read_listed race: {replaced} replaced, {removed} removed");
}

#[test]
fn record_commit_lost_writes_no_fence_once_the_marker_names_another_generation() {
    let (_scratch, root) = store("fence-generation");
    let binding = Binding::capture(&root, true, crate::tests::lease::lease_port()).unwrap();
    let current = crate::wal::marker(&binding).unwrap().generation;
    let other = "f".repeat(32);
    assert_ne!(current, other);
    assert!(matches!(
        fence::record_commit_lost(&binding, &other),
        Error::CommitLost
    ));
    assert!(
        !root.join("store.fence").exists(),
        "no fence for a stale generation"
    );
    assert!(matches!(
        fence::record_commit_lost(&binding, &current),
        Error::CommitLost
    ));
    assert_eq!(
        fs::read(root.join("store.fence")).unwrap(),
        format!("harflow-fence-1\n{current}\ncommit_lost\n").into_bytes()
    );
    // Another error is passed through without a fence check.
    assert!(matches!(
        fence::on_commit_lost(&binding, &current, Error::Busy),
        Error::Busy
    ));
}
