use super::{faults::Fault, support::*, *};
use hf_application::ctl::state_recover::flow::JournalCodecPort;
use std::process::Command;
const CHILD_ENV: &str = "HARFLOW_B4_FLOW_CHILD";
#[test]
fn abort_helper() {
    let _serial = serial();
    let Ok(at) = std::env::var(CHILD_ENV) else {
        return;
    };
    let f = Fixture(PathBuf::from("store"));
    // The child uses an absolute root, while all abort artifacts stay in the fixture.
    let root = fs::canonicalize(&f.0).unwrap();
    std::mem::forget(f);
    let f = Fixture(root);
    let p = f.proposal();
    let s = f.session(&p);
    let mut fault = Fault::new(at.parse().unwrap(), &f.0);
    fault.abort = true;
    let _ = f.run(&p, &s, &fault, &s);
    panic!("fixed abort boundary was not reached");
}
#[test]
fn subprocess_abort_r3a_r5_r7_reacquires_session_without_sqlite_evidence() {
    let _serial = serial();
    let probe = Fixture::new();
    probe.seed();
    let p = probe.proposal();
    let s = probe.session(&p);
    let trace = Fault::new(0, &probe.0);
    probe.run(&p, &s, &trace, &s).unwrap();
    let calls = trace.calls.borrow();
    // R3a has a durable pending record but no journal. R5 has a durable
    // Prepared skeleton; R7 has moved the marker and reached its next barrier.
    let creates: Vec<_> = calls
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == "file.create")
        .map(|(i, _)| i + 1)
        .collect();
    let before_marker = calls.iter().position(|x| *x == "entry.rename").unwrap() + 1;
    let mut points = vec![
        (creates[1], false),
        (before_marker, true),
        (calls.len(), true),
    ];
    points.extend(
        calls
            .iter()
            .enumerate()
            .filter(|(_, name)| matches!(**name, "entry.root_sync" | "entry.data_sync"))
            .map(|(index, _)| (index + 1, true)),
    );
    eprintln!("B4 persisted abort boundaries={points:?}");
    for (at, journaled) in points {
        let f = Fixture::new();
        f.seed();
        let p = f.proposal();
        let items = originals(&f.0);
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "restart::abort_helper", "--nocapture"])
            .env(CHILD_ENV, at.to_string())
            .current_dir(f.0.parent().unwrap())
            .status()
            .unwrap();
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            status.signal(),
            Some(6),
            "child must abort at requested hook"
        );
        preserved(&f.0, Path::new(&p.backup_path()), &items);
        let session = f.session(&p);
        let result = if journaled {
            f.run(&p, &session, &NoArchiveFault, &NoEvidence)
        } else {
            assert!(!f.0.join(".harflow-recovery.json").exists());
            assert!(f.0.join("store.recovery").exists());
            f.run(&p, &session, &NoArchiveFault, &session)
        }
        .unwrap();
        assert_eq!(result.journal.phase, JournalPhase::BackedUp);
        let bytes = fs::read(f.0.join(".harflow-recovery.json")).unwrap();
        assert_eq!(JsonJournalCodec.decode(&bytes).unwrap(), result.journal);
        preserved(&f.0, Path::new(&p.backup_path()), &items);
    }
}
