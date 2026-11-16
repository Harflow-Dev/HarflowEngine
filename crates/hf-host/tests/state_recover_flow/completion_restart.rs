use super::{
    completion_faults::{archived, trace},
    completion_support::NoInitialize,
    faults::Fault,
    support::*,
    *,
};
use hf_platform::durability::FullFlush;
use hf_storage::RecoveryInitializer;
use std::process::Command;
const CHILD: &str = "HARFLOW_B5_ABORT";
#[test]
fn abort_helper() {
    let Ok(at) = std::env::var(CHILD) else {
        return;
    };
    let path = fs::canonicalize("store").unwrap();
    let f = Fixture(path);
    // Recover the original confirmation from the archived journal, never inspect SQLite.
    use hf_application::ctl::state_recover::flow::JournalCodecPort;
    let p = JsonJournalCodec
        .decode(&fs::read(f.0.join(".harflow-recovery.json")).unwrap())
        .unwrap()
        .proposal;
    let s = f.session(&p);
    if at == "init" {
        #[derive(Debug)]
        struct AbortFlush;
        impl hf_application::durability::FlushPort for AbortFlush {
            fn flush(&self, _: &fs::File) -> std::io::Result<()> {
                std::process::abort()
            }
        }
        let _ = f.complete(
            &p,
            &s,
            &NoArchiveFault,
            &NoEvidence,
            &RecoveryInitializer::new(&s, &AbortFlush),
        );
        panic!("initializer abort not reached");
    }
    if at == "cleanup" {
        struct AbortCleanup(std::cell::Cell<usize>);
        impl hf_platform::ctl::archive_fs::ArchiveFaultPort for AbortCleanup {
            fn before(&self, name: &'static str) -> std::io::Result<()> {
                if name == "staging.cleanup_unlink" {
                    let count = self.0.get() + 1;
                    self.0.set(count);
                    if count == 2 {
                        std::process::abort();
                    }
                }
                Ok(())
            }
        }
        let _ = f.complete(
            &p,
            &s,
            &AbortCleanup(std::cell::Cell::new(0)),
            &NoEvidence,
            &RecoveryInitializer::new(&s, &FullFlush),
        );
        panic!("cleanup abort not reached");
    }
    let mut fault = Fault::new(at.parse().unwrap(), &f.0);
    fault.abort = true;
    let _ = f.complete(
        &p,
        &s,
        &fault,
        &NoEvidence,
        &RecoveryInitializer::new(&s, &FullFlush),
    );
    panic!("abort point not reached");
}
#[test]
fn sigabrt_at_every_staged_publish_prefix_resumes_without_sqlite() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = serial();
    let calls = trace();
    for (index, name) in calls.iter().enumerate().filter(|(_, n)| {
        matches!(
            **n,
            "staging.rename"
                | "staging.rename_source_sync"
                | "staging.rename_root_sync"
                | "staging.rmdir"
                | "staging.remove_root_sync"
                | "receipt.rename"
                | "receipt.directory_sync"
        )
    }) {
        let (f, p) = archived();
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "completion_restart::abort_helper", "--nocapture"])
            .env(CHILD, (index + 1).to_string())
            .current_dir(f.0.parent().unwrap())
            .status()
            .unwrap();
        assert_eq!(status.signal(), Some(6), "{name}");
        let s = f.session(&p);
        f.complete(&p, &s, &NoArchiveFault, &NoEvidence, &NoInitialize)
            .unwrap();
    }
}

#[test]
fn abort_before_staging_journal_and_inside_initializer_restarts_safely() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = serial();
    let calls = trace();
    let after_mkdir = calls
        .iter()
        .position(|n| *n == "staging.create_root_sync")
        .unwrap()
        + 1;
    let before_init = calls
        .iter()
        .position(|n| *n == "staging.initialize")
        .unwrap()
        + 1;
    for at in [
        after_mkdir.to_string(),
        before_init.to_string(),
        "init".into(),
    ] {
        let (f, p) = archived();
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "completion_restart::abort_helper", "--nocapture"])
            .env(CHILD, at)
            .current_dir(f.0.parent().unwrap())
            .status()
            .unwrap();
        assert_eq!(status.signal(), Some(6));
        let s = f.session(&p);
        f.complete(
            &p,
            &s,
            &NoArchiveFault,
            &NoEvidence,
            &RecoveryInitializer::new(&s, &FullFlush),
        )
        .unwrap();
    }
}

#[test]
fn sigabrt_during_proven_partial_initializer_cleanup_restarts() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = serial();
    let (f, p) = archived();
    for at in ["init", "cleanup"] {
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "completion_restart::abort_helper", "--nocapture"])
            .env(CHILD, at)
            .current_dir(f.0.parent().unwrap())
            .status()
            .unwrap();
        assert_eq!(status.signal(), Some(6), "{at}");
    }
    let s = f.session(&p);
    f.complete(
        &p,
        &s,
        &NoArchiveFault,
        &NoEvidence,
        &RecoveryInitializer::new(&s, &FullFlush),
    )
    .unwrap();
}
