use hf_application::ctl::state_recover::{
    binding::{BindingError, HeldSessionPort},
    flow::JournalCodecPort,
    journal::{JournalError, JournalPhase, RecoveryJournal},
};
use hf_domain::ctl::{
    identity::PRODUCT_VERSION,
    recovery_proposal::{
        EntryBody, EntryStat, FileIdentity, Inventory, InventoryEntry, RecoveryProposal,
    },
};
use hf_platform::ctl::archive_fs::{ArchiveFaultPort, FsArchive, MacExclusiveRename};
use std::{
    fs::{self, File},
    io,
    os::unix::fs::MetadataExt,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

pub(super) const OP: &str = "0123456789abcdef0123456789abcdef";
static NEXT: AtomicUsize = AtomicUsize::new(0);

struct TempRoot(PathBuf);
impl TempRoot {
    pub(super) fn new() -> Self {
        let path = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "harflow-archive-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Session {
    root: String,
    root_id: FileIdentity,
    lock_id: FileIdentity,
}
impl HeldSessionPort for Session {
    fn verify_held(&self) -> Result<(), BindingError> {
        Ok(())
    }
    fn state_root(&self) -> &str {
        &self.root
    }
    fn held_root_identity(&self) -> FileIdentity {
        self.root_id
    }
    fn held_lock_identity(&self) -> FileIdentity {
        self.lock_id
    }
}

pub(super) struct Fault {
    pub(super) boundary: Option<&'static str>,
}

#[derive(Default)]
pub(super) struct RecordingFault {
    pub(super) fail_once: Option<&'static str>,
    pub(super) failed: Mutex<bool>,
    pub(super) seen: Mutex<Vec<&'static str>>,
}
impl ArchiveFaultPort for RecordingFault {
    fn before(&self, boundary: &'static str) -> io::Result<()> {
        self.seen.lock().unwrap().push(boundary);
        let mut failed = self.failed.lock().unwrap();
        if self.fail_once == Some(boundary) && !*failed {
            *failed = true;
            Err(io::Error::other("injected one-time boundary failure"))
        } else {
            Ok(())
        }
    }
}
impl ArchiveFaultPort for Fault {
    fn before(&self, boundary: &'static str) -> io::Result<()> {
        if self.boundary == Some(boundary) {
            Err(io::Error::other("injected boundary failure"))
        } else {
            Ok(())
        }
    }
}

#[derive(Default)]
pub(super) struct Codec(Mutex<Option<RecoveryJournal>>);
impl JournalCodecPort for Codec {
    fn encode(&self, journal: &RecoveryJournal) -> Result<Vec<u8>, JournalError> {
        *self.0.lock().unwrap() = Some(journal.clone());
        Ok(b"journal\n".to_vec())
    }
    fn decode(&self, _: &[u8]) -> Result<RecoveryJournal, JournalError> {
        self.0.lock().unwrap().clone().ok_or(JournalError::Invalid)
    }
}

pub(super) struct Fixture {
    _temp: TempRoot,
    pub(super) root: PathBuf,
    pub(super) data: PathBuf,
    session: Session,
    rename: MacExclusiveRename,
    pub(super) codec: Codec,
    pub(super) journal: RecoveryJournal,
    pub(super) original: Vec<u8>,
}
impl Fixture {
    pub(super) fn new() -> Self {
        let temp = TempRoot::new();
        let root = temp.0.join("state");
        fs::create_dir(&root).unwrap();
        let lock = root.join("store.lock");
        File::create(&lock).unwrap();
        let source = root.join("store.identity");
        let original = b"identity bytes remain exact\n".to_vec();
        fs::write(&source, &original).unwrap();
        let root_id = identity(&fs::metadata(&root).unwrap());
        let lock_id = identity(&fs::metadata(&lock).unwrap());
        let backup = PathBuf::from(format!("{}.harflow-backup-{OP}", root.display()));
        fs::create_dir(&backup).unwrap();
        let data = backup.join("data");
        fs::create_dir(&data).unwrap();
        let backup_id = identity(&fs::metadata(&backup).unwrap());
        let data_id = identity(&fs::metadata(&data).unwrap());
        let inventory_entry = file_entry(&fs::metadata(&source).unwrap());
        let inventory = Inventory {
            root: root_id,
            lock: Some(lock_id),
            entries: [("store.identity".into(), inventory_entry)].into(),
        };
        let proposal = RecoveryProposal {
            state_root: root.to_string_lossy().into_owned(),
            engine_version: PRODUCT_VERSION.into(),
            root_identity: root_id,
            lock_identity: Some(lock_id),
            inventory_sha256: "a".repeat(64),
            operation_id: OP.into(),
            prior_journal_sha256: None,
        };
        let journal = RecoveryJournal {
            engine_version: PRODUCT_VERSION.into(),
            state_root: proposal.state_root.clone(),
            operation_id: OP.into(),
            backup_path: proposal.backup_path(),
            proposal,
            root_identity: root_id,
            lock_identity: lock_id,
            inventory,
            created_at: "2026-11-16T00:00:00Z".into(),
            phase: JournalPhase::Prepared,
            backup_identity: Some(backup_id),
            data_identity: Some(data_id),
            previous_journal_bytes: None,
            owner_evidence: hf_application::ctl::state_recover::evidence::OwnerEvidence {
                basis: hf_application::ctl::state_recover::evidence::Basis::Clear,
                checked: Vec::new(),
            },
        };
        Self {
            _temp: temp,
            root: root.clone(),
            data: data.clone(),
            session: Session {
                root: root.to_string_lossy().into_owned(),
                root_id,
                lock_id,
            },
            rename: MacExclusiveRename,
            codec: Codec::default(),
            journal,
            original,
        }
    }
    pub(super) fn archive<'a>(&'a self, fault: &'a dyn ArchiveFaultPort) -> FsArchive<'a> {
        FsArchive::new(&self.session, &self.rename, fault, &self.codec)
    }
    pub(super) fn reset_backup_for_prepare(&mut self) {
        let backup = PathBuf::from(&self.journal.backup_path);
        fs::remove_dir_all(&backup).unwrap();
        self.journal.phase = JournalPhase::Planned;
        self.journal.backup_identity = None;
        self.journal.data_identity = None;
    }
}

fn identity(metadata: &fs::Metadata) -> FileIdentity {
    FileIdentity {
        dev: metadata.dev() as i64,
        ino: metadata.ino(),
    }
}
fn file_entry(metadata: &fs::Metadata) -> InventoryEntry {
    InventoryEntry {
        stat: EntryStat {
            dev: metadata.dev() as i64,
            ino: metadata.ino(),
            mode: metadata.mode(),
            size: metadata.len() as i64,
            mtime_ns: metadata.mtime() * 1_000_000_000 + metadata.mtime_nsec(),
        },
        body: EntryBody::File,
    }
}
