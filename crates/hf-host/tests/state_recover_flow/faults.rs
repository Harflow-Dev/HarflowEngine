use super::{support::*, *};
use hf_platform::ctl::archive_fs::ArchiveFaultPort;
use std::{
    cell::{Cell, RefCell},
    io,
};

pub struct Fault {
    pub at: usize,
    pub calls: RefCell<Vec<&'static str>>,
    pub abort: bool,
    pub root: PathBuf,
    pub marker_first: Cell<bool>,
    pub marker_moved: Cell<bool>,
}
impl Fault {
    pub fn new(at: usize, root: &Path) -> Self {
        Self {
            at,
            calls: RefCell::new(Vec::new()),
            abort: false,
            root: root.into(),
            marker_first: Cell::new(false),
            marker_moved: Cell::new(false),
        }
    }
}
impl ArchiveFaultPort for Fault {
    fn before(&self, boundary: &'static str) -> io::Result<()> {
        let mut calls = self.calls.borrow_mut();
        if boundary == "entry.rename" && !self.marker_first.replace(true) {
            assert!(
                self.root.join("store.identity").exists(),
                "marker must be first"
            );
        }
        if boundary == "entry.root_sync" && !self.marker_moved.replace(true) {
            assert!(
                !self.root.join("store.identity").exists(),
                "first rename must move marker"
            );
            for name in ["nested", "link", "store.sqlite3", "store.fence"] {
                assert!(
                    fs::symlink_metadata(self.root.join(name)).is_ok(),
                    "marker must precede {name}"
                );
            }
        }
        calls.push(boundary);
        if calls.len() == self.at {
            if self.abort {
                std::process::abort();
            }
            return Err(io::Error::other("fixed recovery fault"));
        }
        Ok(())
    }
}
#[test]
fn fresh_archive_keeps_every_byte_inode_and_original_session() {
    let _serial = serial();
    let f = Fixture::new();
    f.seed();
    let p = f.proposal();
    let items = originals(&f.0);
    let root_id = file_identity(&f.0);
    let lock_id = file_identity(&f.0.join("store.lock"));
    let s = f.session(&p);
    let trace = Fault::new(0, &f.0);
    let result = f.run(&p, &s, &trace, &s).unwrap();
    assert_eq!(result.journal.phase, JournalPhase::BackedUp);
    assert!(trace.marker_first.get());
    preserved(&f.0, Path::new(&p.backup_path()), &items);
    assert_eq!(file_identity(&f.0), root_id);
    assert_eq!(file_identity(&f.0.join("store.lock")), lock_id);
    let record = fs::read(f.0.join("store.recovery")).unwrap();
    assert!(
        hf_domain::ctl::state_recovery::StoreRecovery::decode(&record)
            .unwrap()
            .is_pending()
    );
    assert_eq!(
        f.run(&p, &s, &NoArchiveFault, &NoEvidence)
            .unwrap()
            .journal
            .phase,
        JournalPhase::BackedUp
    );
}
#[test]
fn every_mutating_hook_fault_preserves_originals_and_retry_is_conservative() {
    let _serial = serial();
    let probe = Fixture::new();
    probe.seed();
    let p = probe.proposal();
    let s = probe.session(&p);
    let trace = Fault::new(0, &probe.0);
    probe.run(&p, &s, &trace, &s).unwrap();
    let count = trace.calls.borrow().len();
    assert!(count > 20);
    assert!(trace.calls.borrow().contains(&"entry.rename"));
    let mut resumed = 0;
    let mut refused = 0;
    for at in 1..=count {
        let f = Fixture::new();
        f.seed();
        let p = f.proposal();
        let items = originals(&f.0);
        let s = f.session(&p);
        assert!(
            f.run(&p, &s, &Fault::new(at, &f.0), &s).is_err(),
            "fault {at}"
        );
        preserved(&f.0, Path::new(&p.backup_path()), &items);
        let retry = f.run(&p, &s, &NoArchiveFault, &s);
        if let Ok(done) = retry {
            assert_eq!(done.journal.phase, JournalPhase::BackedUp);
            resumed += 1;
        } else {
            refused += 1;
        }
        preserved(&f.0, Path::new(&p.backup_path()), &items);
    }
    eprintln!(
        "B4 fault sweep boundaries={count} resumed={resumed} conservative_refusals={refused} trace={:?}",
        trace.calls.borrow()
    );
    assert!(resumed > 0);
    assert!(refused > 0, "unqualified partial temporaries must refuse");
}
