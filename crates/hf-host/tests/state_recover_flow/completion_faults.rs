use super::{completion_support::NoInitialize, faults::Fault, support::*, *};
use hf_platform::{ctl::archive_fs::ArchiveFaultPort, durability::FullFlush};
use hf_storage::RecoveryInitializer;
use std::io;
pub struct NamedFault(pub &'static str);
impl ArchiveFaultPort for NamedFault {
    fn before(&self, name: &'static str) -> io::Result<()> {
        if name == self.0 {
            Err(io::Error::other("fixed staging fault"))
        } else {
            Ok(())
        }
    }
}
pub(super) fn archived() -> (Fixture, hf_domain::ctl::recovery_proposal::RecoveryProposal) {
    let f = Fixture::new();
    f.seed();
    let p = f.proposal();
    let s = f.session(&p);
    f.run(&p, &s, &NoArchiveFault, &s).unwrap();
    drop(s);
    (f, p)
}
pub(super) fn trace() -> Vec<&'static str> {
    let (f, p) = archived();
    let s = f.session(&p);
    let fault = Fault::new(0, &f.0);
    f.complete(
        &p,
        &s,
        &fault,
        &NoEvidence,
        &RecoveryInitializer::new(&s, &FullFlush),
    )
    .unwrap();
    fault.calls.into_inner()
}
#[test]
fn every_staging_publication_commit_and_receipt_hook_fault_is_conservative() {
    let _serial = serial();
    let calls = trace();
    for required in [
        "staging.mkdir",
        "staging.initialize",
        "staging.rename",
        "staging.rename_source_sync",
        "staging.rename_root_sync",
        "staging.rmdir",
        "staging.remove_root_sync",
        "receipt.rename",
        "receipt.directory_sync",
    ] {
        assert!(calls.contains(&required), "missing {required}");
    }
    let mut resumed = 0;
    let mut refused = 0;
    for at in 1..=calls.len() {
        // Darwin intentionally retains descriptor sentinels for process lifetime.
        // Each exhaustive case needs a fresh process to stay within its fixed cap.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "completion_faults::fault_case_helper",
                "--nocapture",
            ])
            .env("HARFLOW_B5_FAULT_AT", at.to_string())
            .env("HARFLOW_B5_FAULT_BOUNDARY", calls[at - 1])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "hook {at}: {}\n{}",
            calls[at - 1],
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        resumed += usize::from(stderr.contains("B5_CASE_RESUMED"));
        refused += usize::from(stderr.contains("B5_CASE_REFUSED"));
    }
    eprintln!(
        "B5 fault sweep boundaries={} resumed={resumed} refused={refused} trace={calls:?}",
        calls.len()
    );
    assert_eq!(resumed + refused, calls.len());
    assert!(resumed > 0);
    assert!(refused > 0);
}

#[test]
fn fault_case_helper() {
    let Ok(at) = std::env::var("HARFLOW_B5_FAULT_AT") else {
        return;
    };
    let at: usize = at.parse().unwrap();
    let boundary = std::env::var("HARFLOW_B5_FAULT_BOUNDARY").unwrap();
    let (f, p) = archived();
    let s = f.session(&p);
    assert!(
        f.complete(
            &p,
            &s,
            &Fault::new(at, &f.0),
            &NoEvidence,
            &RecoveryInitializer::new(&s, &FullFlush)
        )
        .is_err(),
        "hook {at}: {}",
        boundary
    );
    // Interrupted publications can conservatively refuse; successful retries must
    // produce a committed, openable root with its old data only in the backup.
    let retry = f.complete(
        &p,
        &s,
        &NoArchiveFault,
        &NoEvidence,
        &RecoveryInitializer::new(&s, &FullFlush),
    );
    if retry.is_ok() {
        eprintln!("B5_CASE_RESUMED");
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
    }
    if retry.is_err() {
        eprintln!("B5_CASE_REFUSED");
        assert_eq!(
            boundary.as_str(),
            "file.write",
            "unexpected refusal at {at}: {:?}",
            retry
        );
    }
    assert_eq!(
        fs::read(PathBuf::from(p.backup_path()).join("data/nested/payload")).unwrap(),
        b"original payload\0\xff"
    );
}

#[test]
fn unfinished_staging_unknown_symlink_or_replaced_directory_is_never_deleted() {
    let _serial = serial();
    for mode in 0..3 {
        let (f, p) = archived();
        let s = f.session(&p);
        assert!(
            f.complete(
                &p,
                &s,
                &NamedFault("staging.initialize"),
                &NoEvidence,
                &RecoveryInitializer::new(&s, &FullFlush)
            )
            .is_err()
        );
        let staging =
            f.0.join(format!(".harflow-recovery-init-{}", p.operation_id));
        if mode == 2 {
            fs::rename(&staging, staging.with_extension("retained")).unwrap();
            fs::create_dir(&staging).unwrap();
        }
        let unknown = staging.join(if mode == 0 {
            "unknown"
        } else {
            "store.sqlite3"
        });
        if mode == 1 {
            std::os::unix::fs::symlink("../store.lock", &unknown).unwrap();
        } else {
            fs::write(&unknown, b"retain").unwrap();
        }
        let id = file_identity(&unknown);
        assert_eq!(
            f.complete(&p, &s, &NoArchiveFault, &NoEvidence, &NoInitialize),
            Err(hf_application::ctl::state_recover::flow::RecoveryError::BackupConflict)
        );
        assert_eq!(file_identity(&unknown), id);
    }
}

#[test]
fn missing_replaced_or_reserved_root_descendant_identity_refuses_without_deletion() {
    let _serial = serial();
    for mode in 0..4 {
        let (f, p) = archived();
        let original = PathBuf::from(p.backup_path()).join("data/nested/payload");
        let retained = f.0.parent().unwrap().join("retained-original");
        let reserved = f.0.join("store.recovery.tmp");
        let original_id = file_identity(&original);
        match mode {
            0 | 1 => {
                fs::rename(&original, &retained).unwrap();
                if mode == 1 {
                    fs::write(&original, b"replacement").unwrap();
                }
            }
            2 => {
                fs::rename(&original, &reserved).unwrap();
            }
            _ => {
                fs::hard_link(&original, &reserved).unwrap();
            }
        }
        let journal = fs::read(f.0.join(".harflow-recovery.json")).unwrap();
        let record = fs::read(f.0.join("store.recovery")).unwrap();
        let s = f.session(&p);
        assert_eq!(
            f.complete(&p, &s, &NoArchiveFault, &NoEvidence, &NoInitialize),
            Err(hf_application::ctl::state_recover::flow::RecoveryError::BackupConflict)
        );
        assert_eq!(
            fs::read(f.0.join(".harflow-recovery.json")).unwrap(),
            journal
        );
        assert_eq!(fs::read(f.0.join("store.recovery")).unwrap(), record);
        let at = if mode < 2 { &retained } else { &reserved };
        assert_eq!(file_identity(at), original_id);
        assert_eq!(fs::read(at).unwrap(), b"original payload\0\xff");
        if mode == 1 {
            assert_eq!(fs::read(&original).unwrap(), b"replacement");
        }
        if mode == 3 {
            assert_eq!(file_identity(&original), original_id);
        }
    }
}
