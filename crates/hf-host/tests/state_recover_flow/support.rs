use super::*;
use hf_application::ctl::state_recover::{
    evidence::OwnerEvidencePort,
    flow::{BackedUpHandoff, RecoveryError},
};
use hf_domain::ctl::recovery_proposal::RecoveryProposal;
use hf_platform::ctl::archive_fs::ArchiveFaultPort;
pub(super) struct Fixture(pub PathBuf);
impl Fixture {
    pub fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "hf-state-recover-flow-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
        fs::create_dir(&path).unwrap();
        Self(path.join("store"))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0.parent().unwrap());
    }
}

struct FixedIds;
impl OperationIdPort for FixedIds {
    fn fresh(&self) -> Option<String> {
        Some("0123456789abcdef0123456789abcdef".into())
    }
}

pub fn file_identity(path: &Path) -> hf_domain::capture::FileIdentity {
    let metadata = fs::symlink_metadata(path).unwrap();
    hf_domain::capture::FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

impl Fixture {
    pub fn seed(&self) {
        let store = Store::initialize_fixture(
            &self.0,
            Limits::default(),
            Instant::now() + Duration::from_secs(5),
            hf_platform::lease::shared(),
        )
        .unwrap();
        store
            .apply_batch(
                &[Mutation::Put {
                    key: "k".into(),
                    value: b"value".to_vec(),
                }],
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        let generation = store.identity().generation.clone();
        drop(store);
        fs::write(
            self.0.join("store.fence"),
            format!("harflow-fence-1\n{generation}\nfailed\n"),
        )
        .unwrap();
        fs::create_dir(self.0.join("nested")).unwrap();
        fs::write(self.0.join("nested/payload"), b"original payload\0\xff").unwrap();
        std::os::unix::fs::symlink("nested/payload", self.0.join("link")).unwrap();
    }
    pub fn root(&self) -> StateRoot {
        StateRoot::from_normalized(self.0.to_string_lossy().into_owned())
    }
    pub fn proposal(&self) -> RecoveryProposal {
        inspect_state(
            &FsStateObserver,
            &JsonStateCodec,
            policy(),
            &ProposalPorts {
                inventory: &FsInventory,
                codec: &JsonRecoveryCodec,
                digest: &Sha256Digest,
                ids: &FixedIds,
            },
            &self.root(),
        )
        .recovery
        .unwrap()
        .proposal
    }
    pub fn session(&self, p: &RecoveryProposal) -> MaintenanceSession {
        MaintenanceSession::acquire_within(
            &self.0,
            &SessionRequest {
                root_identity: file_identity(&self.0),
                lock_identity: p.lock_identity.map(|i| hf_domain::capture::FileIdentity {
                    device: i.dev as u64,
                    inode: i.ino,
                }),
            },
            Arc::new(MacRecoveryFs),
            hf_platform::lease::shared(),
            Duration::from_secs(2),
        )
        .unwrap()
    }
    pub fn run(
        &self,
        p: &RecoveryProposal,
        s: &MaintenanceSession,
        faults: &dyn ArchiveFaultPort,
        evidence: &dyn OwnerEvidencePort,
    ) -> Result<BackedUpHandoff, RecoveryError> {
        self.run_with_identities(p, s, faults, evidence, &hf_platform::process::MacIdentity)
    }
    pub fn run_with_identities(
        &self,
        p: &RecoveryProposal,
        s: &MaintenanceSession,
        faults: &dyn ArchiveFaultPort,
        evidence: &dyn OwnerEvidencePort,
        identities: &dyn hf_application::process::IdentityPort,
    ) -> Result<BackedUpHandoff, RecoveryError> {
        let archive = FsArchive::new(s, &MacExclusiveRename, faults, &JsonJournalCodec);
        recover(
            &RecoveryRequest {
                root: &self.root(),
                proposal: p,
                operation_id: &p.operation_id,
                created_at: "2026-11-16T00:00:00Z",
            },
            &RecoveryPorts {
                session: s,
                evidence,
                observer: &FsStateObserver,
                state_codec: &JsonStateCodec,
                policy: policy(),
                inventory: &FsInventory,
                inventory_codec: &JsonRecoveryCodec,
                digest: &Sha256Digest,
                identities,
                journal_codec: &JsonJournalCodec,
                mutations: &archive,
            },
        )
    }
}
pub fn policy() -> StorePolicy {
    StorePolicy {
        schema_version: hf_storage::SCHEMA_VERSION,
        application_id: hf_storage::APPLICATION_ID,
    }
}
pub struct NoEvidence;
impl OwnerEvidencePort for NoEvidence {
    fn read(
        &self,
        _: &str,
    ) -> Result<
        hf_application::ctl::state_recover::evidence::EvidenceSet,
        hf_application::ctl::state_recover::evidence::EvidenceError,
    > {
        panic!("resume opened SQLite evidence")
    }
}
#[derive(Debug)]
pub struct Original {
    pub relative: PathBuf,
    pub identity: hf_domain::capture::FileIdentity,
    pub bytes: Option<Vec<u8>>,
    pub link: Option<PathBuf>,
}
pub fn originals(root: &Path) -> Vec<Original> {
    fn walk(base: &Path, at: &Path, out: &mut Vec<Original>) {
        for e in fs::read_dir(at).unwrap() {
            let p = e.unwrap().path();
            let name = p.file_name().unwrap().to_str().unwrap();
            if at == base && hf_domain::ctl::recovery_proposal::INVENTORY_EXCLUDED.contains(&name) {
                continue;
            }
            let m = fs::symlink_metadata(&p).unwrap();
            out.push(Original {
                relative: p.strip_prefix(base).unwrap().into(),
                identity: file_identity(&p),
                bytes: if m.is_file() {
                    Some(fs::read(&p).unwrap())
                } else {
                    None
                },
                link: if m.file_type().is_symlink() {
                    Some(fs::read_link(&p).unwrap())
                } else {
                    None
                },
            });
            if m.is_dir() {
                walk(base, &p, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}
pub fn preserved(root: &Path, backup: &Path, items: &[Original]) {
    for e in items {
        let a = root.join(&e.relative);
        let b = backup.join("data").join(&e.relative);
        let aa = fs::symlink_metadata(&a).is_ok();
        let bb = fs::symlink_metadata(&b).is_ok();
        assert_ne!(aa, bb, "entry must exist exactly once: {:?}", e.relative);
        let p = if aa { a } else { b };
        assert_eq!(file_identity(&p), e.identity);
        if let Some(bytes) = &e.bytes {
            assert_eq!(&fs::read(&p).unwrap(), bytes);
        }
        if let Some(link) = &e.link {
            assert_eq!(&fs::read_link(p).unwrap(), link);
        }
    }
}

// The production Darwin coordinator deliberately uses one process-wide try_lock.
// Separate roots can therefore return Busy when test threads acquire concurrently.
// Keep these actual-session scenarios independent without changing recovery retries.
pub fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
