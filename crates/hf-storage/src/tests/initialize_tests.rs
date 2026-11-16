#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use crate::wal_owner::SyncAllFlush;
use crate::{Limits, Store, WriterOptions, initialize_files};
use hf_application::{
    durability::FlushPort,
    lease::{LeaseError, LeaseGuard, LeaseMode, LeasePort},
};
use rusqlite::{Connection, OpenFlags};
use std::{
    cell::Cell,
    fs::{self, File},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

struct Guard {
    _guard: Box<dyn LeaseGuard>,
}
impl LeaseGuard for Guard {}

struct TraceLease(Arc<Mutex<Vec<String>>>);
impl LeasePort for TraceLease {
    fn acquire(
        &self,
        path: &Path,
        mode: LeaseMode,
        create: bool,
        deadline: Instant,
    ) -> Result<Box<dyn LeaseGuard>, LeaseError> {
        self.0.lock().unwrap().push(format!(
            "lease:{}:{mode:?}:{create}",
            path.file_name().unwrap().to_string_lossy()
        ));
        super::lease::FileLeasePort
            .acquire(path, mode, create, deadline)
            .map(|guard| Box::new(Guard { _guard: guard }) as Box<dyn LeaseGuard>)
    }
}

#[derive(Debug, Default)]
struct TraceFlush {
    calls: AtomicUsize,
    trace: Arc<Mutex<Vec<String>>>,
}
impl FlushPort for TraceFlush {
    fn flush(&self, file: &File) -> std::io::Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.trace.lock().unwrap().push("flush:watermark".into());
        file.sync_all()
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

fn scratch(name: &str) -> std::path::PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "hf-storage-init-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir(&path).unwrap();
    fs::canonicalize(path).unwrap()
}

#[test]
fn bare_initializer_creates_only_fixed_files_and_derived_shm_in_an_existing_directory() {
    let parent = scratch("bare");
    let missing = parent.join("missing");
    assert!(initialize_files(&missing, &SyncAllFlush, deadline(), &|| Ok(())).is_err());
    assert!(!missing.exists());
    let root = parent.join("stage");
    fs::create_dir(&root).unwrap();
    initialize_files(&root, &SyncAllFlush, deadline(), &|| Ok(())).unwrap();
    let mut names = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        [
            "store.identity",
            "store.sqlite3",
            "store.sqlite3-shm",
            "store.sqlite3-wal",
            "store.watermark"
        ]
    );
    assert!(
        fs::metadata(root.join("store.sqlite3-wal")).unwrap().len() > 0,
        "fresh WAL must carry frames through staged renames"
    );
    assert!(!root.join("store.lock").exists());
    assert!(!root.join("io.lock").exists());
    let identity = crate::wal::read_marker_at(&root, true).unwrap();
    assert_eq!(
        crate::watermark::read(&root, &identity.generation).unwrap(),
        0
    );
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn recovery_keeps_wal_frames_while_fixture_initializer_truncates_before_open() {
    let parent = scratch("checkpoint-policies");
    let recovery_root = parent.join("recovery");
    fs::create_dir(&recovery_root).unwrap();
    let recovery_identity =
        initialize_files(&recovery_root, &SyncAllFlush, deadline(), &|| Ok(())).unwrap();
    assert!(
        fs::metadata(recovery_root.join("store.sqlite3-wal"))
            .unwrap()
            .len()
            > 0
    );
    assert_eq!(
        crate::wal::read_marker_at(&recovery_root, true).unwrap(),
        recovery_identity
    );
    assert_eq!(
        crate::watermark::read(&recovery_root, &recovery_identity.generation).unwrap(),
        0
    );

    let fixture_root = parent.join("fixture");
    fs::create_dir(&fixture_root).unwrap();
    File::create(fixture_root.join("store.lock")).unwrap();
    File::create(fixture_root.join("io.lock")).unwrap();
    let truncated_before_open = Cell::new(None);
    let fixture_identity =
        crate::initialize::fixture_files(&fixture_root, &SyncAllFlush, deadline(), &|| {
            if fixture_root.join("store.identity").exists()
                && fixture_root.join("store.watermark").exists()
            {
                truncated_before_open.set(Some(
                    fs::metadata(fixture_root.join("store.sqlite3-wal"))
                        .unwrap()
                        .len(),
                ));
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(truncated_before_open.get(), Some(0));
    assert_eq!(
        crate::wal::read_marker_at(&fixture_root, true).unwrap(),
        fixture_identity
    );
    assert_eq!(
        crate::watermark::read(&fixture_root, &fixture_identity.generation).unwrap(),
        0
    );
    let opened = Store::open(
        &fixture_root,
        Limits::default(),
        deadline(),
        super::lease::lease_port(),
    )
    .unwrap();
    assert_eq!(opened.identity().generation, fixture_identity.generation);
    drop(opened);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn bare_initializer_refuses_symlinked_database_without_touching_target() {
    #[cfg(unix)]
    {
        let parent = scratch("nofollow");
        let root = parent.join("stage");
        fs::create_dir(&root).unwrap();
        let target = parent.join("outside.sqlite3");
        fs::write(&target, b"untouched target").unwrap();
        std::os::unix::fs::symlink(&target, root.join("store.sqlite3")).unwrap();
        assert!(initialize_files(&root, &SyncAllFlush, deadline(), &|| Ok(())).is_err());
        assert_eq!(fs::read(target).unwrap(), b"untouched target");
        assert!(!root.join("store.identity").exists());
        fs::remove_dir_all(parent).unwrap();
    }
}

#[test]
fn failed_watermark_flush_closes_database_and_keeps_marker_unpublished() {
    #[derive(Debug)]
    struct FailFlush;
    impl FlushPort for FailFlush {
        fn flush(&self, _: &File) -> std::io::Result<()> {
            Err(std::io::Error::other("injected flush failure"))
        }
    }
    let parent = scratch("flush-failure");
    let root = parent.join("stage");
    fs::create_dir(&root).unwrap();
    assert!(initialize_files(&root, &FailFlush, deadline(), &|| Ok(())).is_err());
    assert!(!root.join("store.identity").exists());
    let connection =
        Connection::open_with_flags(root.join("store.sqlite3"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let schema: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(schema, crate::SCHEMA_VERSION);
    drop(connection);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn fixture_initializer_keeps_directory_lock_file_order_and_open_after_close() {
    let parent = scratch("fixture-order");
    let root = parent.join("store");
    let trace = Arc::new(Mutex::new(Vec::new()));
    let flush = Arc::new(TraceFlush {
        calls: AtomicUsize::new(0),
        trace: trace.clone(),
    });
    let leases = Arc::new(TraceLease(trace.clone()));
    let options = WriterOptions::suppressed().with_flush(flush.clone());
    let store = Store::initialize_fixture_with(
        &root,
        Limits::default(),
        deadline(),
        leases.clone(),
        options,
    )
    .unwrap();
    assert_eq!(store.identity().schema_version, crate::SCHEMA_VERSION);
    drop(store);
    let actual = trace.lock().unwrap().clone();
    assert_eq!(
        actual,
        [
            "lease:store.lock:Exclusive:true",
            "lease:io.lock:Exclusive:true",
            "flush:watermark",
            "lease:store.lock:Shared:false",
            "lease:io.lock:Exclusive:false",
        ]
    );
    assert_eq!(flush.calls.load(Ordering::SeqCst), 1);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn expired_deadline_creates_no_database_or_sidecars() {
    let parent = scratch("deadline");
    let root = parent.join("stage");
    fs::create_dir(&root).unwrap();
    assert!(
        initialize_files(
            &root,
            &SyncAllFlush,
            Instant::now() - Duration::from_millis(1),
            &|| Ok(())
        )
        .is_err()
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    fs::remove_dir_all(parent).unwrap();
}
