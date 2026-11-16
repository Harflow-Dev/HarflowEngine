use super::{faults::Fault, support::*, *};
fn interrupted() -> (
    Fixture,
    hf_domain::ctl::recovery_proposal::RecoveryProposal,
    Vec<Original>,
) {
    let probe = Fixture::new();
    probe.seed();
    let p = probe.proposal();
    let s = probe.session(&p);
    let trace = Fault::new(0, &probe.0);
    probe.run(&p, &s, &trace, &s).unwrap();
    let at = trace
        .calls
        .borrow()
        .iter()
        .position(|b| *b == "entry.rename")
        .unwrap()
        + 2;
    let f = Fixture::new();
    f.seed();
    let p = f.proposal();
    let items = originals(&f.0);
    let s = f.session(&p);
    assert!(f.run(&p, &s, &Fault::new(at, &f.0), &s).is_err());
    drop(s);
    (f, p, items)
}
#[test]
fn unexpected_entry_and_double_location_refuse_without_overwrite() {
    let _serial = serial();
    for duplicate in [false, true] {
        let (f, p, items) = interrupted();
        let backup = PathBuf::from(p.backup_path());
        if duplicate {
            fs::write(f.0.join("store.identity"), b"intruder").unwrap();
        } else {
            fs::write(f.0.join("unexpected"), b"intruder").unwrap();
        }
        let s = f.session(&p);
        assert!(f.run(&p, &s, &NoArchiveFault, &NoEvidence).is_err());
        let inserted = f.0.join(if duplicate {
            "store.identity"
        } else {
            "unexpected"
        });
        assert_eq!(fs::read(inserted).unwrap(), b"intruder");
        if !duplicate {
            preserved(&f.0, &backup, &items);
        } else {
            let marker = items
                .iter()
                .find(|e| e.relative == Path::new("store.identity"))
                .unwrap();
            assert_eq!(
                file_identity(&backup.join("data/store.identity")),
                marker.identity
            );
        }
    }
}
#[test]
fn replaced_backup_and_symlink_refuse_without_touching_original_archive() {
    let _serial = serial();
    for symlink in [false, true] {
        let (f, p, items) = interrupted();
        let backup = PathBuf::from(p.backup_path());
        let saved = backup.with_extension("saved");
        fs::rename(&backup, &saved).unwrap();
        if symlink {
            std::os::unix::fs::symlink(&saved, &backup).unwrap();
        } else {
            fs::create_dir(&backup).unwrap();
            fs::write(backup.join("sentinel"), b"replacement").unwrap();
        }
        let s = f.session(&p);
        assert!(f.run(&p, &s, &NoArchiveFault, &NoEvidence).is_err());
        preserved(&f.0, &saved, &items);
        assert!(fs::symlink_metadata(&backup).is_ok());
    }
}
#[test]
fn substituted_root_and_ancestor_are_fenced_by_original_session() {
    let _serial = serial();
    for (ancestor, symlink) in [(false, false), (true, false), (false, true), (true, true)] {
        let f = Fixture::new();
        f.seed();
        let p = f.proposal();
        let s = f.session(&p);
        let source = if ancestor {
            f.0.parent().unwrap().to_path_buf()
        } else {
            f.0.clone()
        };
        let saved = source.with_extension("saved");
        fs::rename(&source, &saved).unwrap();
        if symlink {
            std::os::unix::fs::symlink(&saved, &source).unwrap();
        } else {
            fs::create_dir(&source).unwrap();
            if ancestor {
                fs::create_dir(&f.0).unwrap();
            }
            fs::write(f.0.join("sentinel"), b"replacement").unwrap();
        }
        assert!(f.run(&p, &s, &NoArchiveFault, &s).is_err());
        if symlink {
            assert!(
                fs::symlink_metadata(&source)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        } else {
            assert_eq!(fs::read(f.0.join("sentinel")).unwrap(), b"replacement");
        }
        drop(s);
        if symlink {
            fs::remove_file(&source).unwrap();
        } else {
            fs::remove_dir_all(&source).unwrap();
        }
        fs::rename(&saved, &source).unwrap();
    }
}

#[test]
fn recorded_live_and_unknown_identity_resume_refuses_without_sqlite() {
    let _serial = serial();
    use hf_application::{
        ctl::state_recover::flow::{JournalCodecPort, RecoveryError},
        process::IdentityPort,
    };
    use hf_domain::process::{IdentityObservation, ProcessIdentity};
    struct Identity(IdentityObservation);
    impl IdentityPort for Identity {
        fn identity(&self, _: u32) -> Option<ProcessIdentity> {
            None
        }
        fn observe(&self, _: &ProcessIdentity) -> IdentityObservation {
            self.0
        }
    }
    for (observation, expected) in [
        (IdentityObservation::Matching, RecoveryError::ActiveOwner),
        (IdentityObservation::Unknown, RecoveryError::AmbiguousOwner),
    ] {
        let (f, p, items) = interrupted();
        let path = f.0.join(".harflow-recovery.json");
        let mut journal = JsonJournalCodec.decode(&fs::read(&path).unwrap()).unwrap();
        journal.owner_evidence.checked.push(
            ProcessIdentity::new(12345, 10, 0, "abcdef01-2345-6789-abcd-ef0123456789".into())
                .unwrap(),
        );
        fs::write(path, JsonJournalCodec.encode(&journal).unwrap()).unwrap();
        let s = f.session(&p);
        assert_eq!(
            f.run_with_identities(&p, &s, &NoArchiveFault, &NoEvidence, &Identity(observation)),
            Err(expected)
        );
        preserved(&f.0, Path::new(&p.backup_path()), &items);
    }
}
