pub use super::fault_trace::Fault;
use super::{support::*, *};

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
