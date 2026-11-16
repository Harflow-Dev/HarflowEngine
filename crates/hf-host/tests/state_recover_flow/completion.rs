use super::{completion_support::NoInitialize, faults::Fault, support::*, *};
use hf_application::ctl::state_recover::{completion::completed_result, flow::JournalCodecPort};
use hf_platform::{ctl::archive_fs::FsRecoveryRead, durability::FullFlush};
use hf_storage::RecoveryInitializer;

#[test]
fn wal_frames_survive_rename_and_shm_removal_and_receipt_is_lease_free() {
    let _serial = serial();
    let f = Fixture::new();
    f.seed();
    let p = f.proposal();
    let s = f.session(&p);
    let items = originals(&f.0);
    f.run(&p, &s, &NoArchiveFault, &s).unwrap();
    preserved(&f.0, Path::new(&p.backup_path()), &items);
    let result = f
        .complete(
            &p,
            &s,
            &NoArchiveFault,
            &s,
            &RecoveryInitializer::new(&s, &FullFlush),
        )
        .unwrap();
    assert!(!f.0.join("store.sqlite3-shm").exists());
    assert!(fs::metadata(f.0.join("store.sqlite3-wal")).unwrap().len() > 0);
    let journal = JsonJournalCodec
        .decode(&fs::read(f.0.join(".harflow-recovery.json")).unwrap())
        .unwrap();
    assert_eq!(journal.phase, JournalPhase::Completed);
    drop(s);
    let store = Store::open(
        &f.0,
        Limits::default(),
        Instant::now() + Duration::from_secs(5),
        hf_platform::lease::shared(),
    )
    .unwrap();
    assert!(
        store
            .read_batch(&["k".into()], Instant::now() + Duration::from_secs(5))
            .unwrap()
            .is_empty()
    );
    put_later(&store);
    let root = f.root();
    let request = RecoveryRequest {
        root: &root,
        proposal: &p,
        operation_id: &p.operation_id,
        created_at: "2026-11-16T00:00:00Z",
    };
    assert_eq!(
        completed_result(
            &request,
            &FsRecoveryRead::new(&root, &JsonStateCodec),
            &JsonJournalCodec
        )
        .unwrap(),
        Some(result.clone())
    );
    drop(store);
    fs::write(f.0.join("legitimate-current-entry"), b"retained").unwrap();
    let s = f.session(&p);
    assert_eq!(
        f.complete(&p, &s, &NoArchiveFault, &NoEvidence, &NoInitialize)
            .unwrap(),
        result
    );
    drop(s);
    let store = Store::open(
        &f.0,
        Limits::default(),
        Instant::now() + Duration::from_secs(5),
        hf_platform::lease::shared(),
    )
    .unwrap();
    assert_eq!(
        store
            .read_batch(&["later".into()], Instant::now() + Duration::from_secs(5))
            .unwrap()[0]
            .value,
        b"legitimate"
    );
}

pub fn staged_fixture() -> (Fixture, hf_domain::ctl::recovery_proposal::RecoveryProposal) {
    let f = Fixture::new();
    f.seed();
    let p = f.proposal();
    let s = f.session(&p);
    f.run(&p, &s, &NoArchiveFault, &s).unwrap();
    let fault = super::completion_faults::NamedFault("staging.rename");
    assert!(
        f.complete(
            &p,
            &s,
            &fault,
            &NoEvidence,
            &RecoveryInitializer::new(&s, &FullFlush)
        )
        .is_err()
    );
    drop(s);
    (f, p)
}

#[test]
fn staged_resume_refuses_changed_bytes_zero_and_both_locations_without_deletion() {
    let _serial = serial();
    for mode in 0..7 {
        let (f, p) = staged_fixture();
        let staging =
            f.0.join(format!(".harflow-recovery-init-{}", p.operation_id));
        corrupt_staged_watermark(&f, &staging, mode);
        let before: Vec<_> = fs::read_dir(&staging)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        let s = f.session(&p);
        assert_eq!(
            f.complete(&p, &s, &NoArchiveFault, &NoEvidence, &NoInitialize),
            Err(hf_application::ctl::state_recover::flow::RecoveryError::BackupConflict)
        );
        let after: Vec<_> = fs::read_dir(&staging)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(before, after);
    }
}

#[test]
fn partial_and_conflicting_receipts_reconcile_or_refuse() {
    let _serial = serial();
    for conflict in [false, true] {
        let f = Fixture::new();
        f.seed();
        let p = f.proposal();
        let s = f.session(&p);
        let trace = Fault::new(0, &f.0);
        let result = f
            .complete(
                &p,
                &s,
                &trace,
                &s,
                &RecoveryInitializer::new(&s, &FullFlush),
            )
            .unwrap();
        let receipt = PathBuf::from(p.backup_path()).join("receipt.json");
        if conflict {
            fs::write(&receipt, b"{}").unwrap();
        } else {
            fs::rename(&receipt, receipt.with_file_name("receipt.json.pending")).unwrap();
        }
        let retry = f.complete(&p, &s, &NoArchiveFault, &NoEvidence, &NoInitialize);
        if conflict {
            assert_eq!(
                retry,
                Err(hf_application::ctl::state_recover::flow::RecoveryError::BackupConflict)
            );
        } else {
            assert_eq!(retry.unwrap(), result);
        }
    }
}

fn put_later(store: &Store) {
    store
        .apply_batch(
            &[Mutation::Put {
                key: "later".into(),
                value: b"legitimate".to_vec(),
            }],
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
}

fn corrupt_staged_watermark(f: &Fixture, staging: &Path, mode: usize) {
    let source = staging.join("store.watermark");
    let metadata = fs::metadata(&source).unwrap();
    let old_time = metadata.modified().unwrap();
    if mode == 0 {
        let mut bytes = fs::read(&source).unwrap();
        bytes[0] ^= 1;
        fs::write(&source, bytes).unwrap();
        fs::File::options()
            .write(true)
            .open(&source)
            .unwrap()
            .set_modified(old_time)
            .unwrap();
    }
    if mode == 3 {
        fs::OpenOptions::new()
            .write(true)
            .open(&source)
            .unwrap()
            .set_len(metadata.len() + 1)
            .unwrap();
    }
    if mode == 4 {
        fs::File::options()
            .write(true)
            .open(&source)
            .unwrap()
            .set_modified(old_time + Duration::from_secs(10))
            .unwrap();
    }
    if mode == 5 {
        let retained = source.with_extension("retained");
        fs::rename(&source, &retained).unwrap();
        fs::copy(&retained, &source).unwrap();
        fs::remove_file(retained).unwrap();
        fs::File::options()
            .write(true)
            .open(&source)
            .unwrap()
            .set_modified(old_time)
            .unwrap();
    }
    if mode == 1 {
        fs::rename(&source, f.0.parent().unwrap().join("retained-watermark")).unwrap();
    }
    if mode == 2 {
        fs::hard_link(&source, f.0.join("store.watermark")).unwrap();
    }
    if mode == 6 {
        let path = f.0.join(".harflow-recovery.json");
        let mut journal = JsonJournalCodec.decode(&fs::read(&path).unwrap()).unwrap();
        journal
            .staged
            .as_mut()
            .unwrap()
            .get_mut("store.watermark")
            .unwrap()
            .identity
            .dev ^= 1;
        fs::write(path, JsonJournalCodec.encode(&journal).unwrap()).unwrap();
    }
}
