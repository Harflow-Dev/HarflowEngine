#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::*;
use hf_domain::{capture::CaptureProgress, process::ProcessExit};
use std::{
    fs::{self, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::PathBuf,
    sync::mpsc,
    thread,
};

fn publish(
    store: &Store,
    binding: &hf_domain::capture::CaptureBinding,
    revision: u64,
    progress: CaptureProgress,
) {
    store
        .capture_publish(binding, revision - 1, &progress, deadline())
        .unwrap();
}

fn seed_history(store: &Store) -> Id {
    let binding = accepted_binding(store, 71);
    let id = binding.fence.operation.clone();
    store.capture_begin(&binding, deadline()).unwrap();
    publish(
        store,
        &binding,
        1,
        CaptureProgress {
            revision: 1,
            stdout: 1,
            stderr: 0,
            stdout_eof: false,
            stderr_eof: false,
            loss: false,
        },
    );
    publish(
        store,
        &binding,
        2,
        CaptureProgress {
            revision: 2,
            stdout: 5,
            stderr: 4,
            stdout_eof: true,
            stderr_eof: true,
            loss: false,
        },
    );
    store
        .capture_finish(&binding, 2, ProcessExit::Code(7), deadline())
        .unwrap();
    id
}

#[test]
fn valid_different_intermediate_history_changes_full_digest_only() {
    let fixture = Fixture::new("recovery-history");
    let root = fixture.root();
    let store = fixture.store();
    let operation = seed_history(&store);
    drop(store);
    let scratch = root.parent().unwrap().join("scratch");
    fs::create_dir(&scratch).unwrap();
    let session = RecoveryReadSession::new(
        &root,
        &scratch,
        Limits::default(),
        crate::tests::lease::lease_port(),
    );
    let first = session.read(&operation, deadline()).unwrap().unwrap();
    let write = crate::tests::raw_connection(root.join("store.sqlite3"));
    write
        .execute(
            "UPDATE capture_events SET stdout=2 WHERE operation=?1 AND revision=1",
            [operation.as_str()],
        )
        .unwrap();
    drop(write);
    let second = session.read(&operation, deadline()).unwrap().unwrap();
    assert_eq!(first.events_count, second.events_count);
    assert_eq!(first.capture, second.capture);
    assert_ne!(first.events_digest, second.events_digest);
}

#[test]
fn event_gap_reverse_after_eof_and_terminal_mismatch_refuse_without_writes() {
    for (label, sql) in [
        (
            "gap",
            "UPDATE capture_events SET revision=4 WHERE operation=?1 AND revision=1",
        ),
        (
            "reverse",
            "UPDATE capture_events SET stdout=6 WHERE operation=?1 AND revision=1",
        ),
        (
            "after-eof",
            "UPDATE capture_events SET stdout=4 WHERE operation=?1 AND revision=2",
        ),
        (
            "terminal",
            "UPDATE capture_events SET exit_value=8 WHERE operation=?1 AND revision=3",
        ),
    ] {
        check_corruption(label, sql);
    }
}

fn check_corruption(label: &str, sql: &str) {
    let fixture = Fixture::new(&format!("recovery-corrupt-{label}"));
    let root = fixture.root();
    let store = fixture.store();
    let operation = seed_history(&store);
    drop(store);
    let write = crate::tests::raw_connection(root.join("store.sqlite3"));
    write.execute(sql, [operation.as_str()]).unwrap();
    drop(write);
    let before = source_hashes(&root);
    let scratch = root.parent().unwrap().join("scratch");
    fs::create_dir(&scratch).unwrap();
    let session = RecoveryReadSession::new(
        &root,
        &scratch,
        Limits::default(),
        crate::tests::lease::lease_port(),
    );
    assert!(session.read(&operation, deadline()).is_err(), "{label}");
    assert_eq!(before, source_hashes(&root), "{label}");
    assert_eq!(fs::read_dir(scratch).unwrap().count(), 0, "{label}");
}

#[test]
fn combined_main_and_wal_limit_refuses_sparse_wal_without_scratch_residue() {
    let fixture = Fixture::new("recovery-copy-cap");
    let root = fixture.root();
    let store = fixture.store();
    let operation = seed_terminal(&store, 81);
    drop(store);
    let scratch = root.parent().unwrap().join("scratch");
    fs::create_dir(&scratch).unwrap();
    let low = RecoveryReadSession::new(
        &root,
        &scratch,
        Limits {
            max_rows: 1,
            max_bytes: 16,
        },
        crate::tests::lease::lease_port(),
    );
    assert!(low.read(&operation, deadline()).is_err());
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
    let wal = root.join("store.sqlite3-wal");
    let mut file = OpenOptions::new().write(true).open(&wal).unwrap();
    file.set_len(64 * 1024 * 1024 + 1).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(b"sparse-source-cap-fixture").unwrap();
    drop(file);
    let length = fs::metadata(&wal).unwrap().len();
    let header = fs::read(&wal).unwrap()[..32].to_vec();
    let before = source_hashes(&root);
    let full = RecoveryReadSession::new(
        &root,
        &scratch,
        Limits::default(),
        crate::tests::lease::lease_port(),
    );
    assert!(full.read(&operation, deadline()).is_err());
    assert_eq!(before, source_hashes(&root));
    assert_eq!(fs::metadata(&wal).unwrap().len(), length);
    assert_eq!(&fs::read(&wal).unwrap()[..32], header.as_slice());
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
}

#[test]
fn concurrent_independent_readers_finish_bounded_with_success_or_busy() {
    let fixture = Fixture::new("recovery-concurrent");
    let root = fixture.root();
    let store = fixture.store();
    let operation = seed_terminal(&store, 91);
    drop(store);
    let scratch = root.parent().unwrap().join("scratch");
    fs::create_dir(&scratch).unwrap();
    let before = source_hashes(&root);
    let until = deadline();
    let (send, receive) = mpsc::channel();
    let readers = (0..2)
        .map(|_| {
            spawn_reader(
                root.clone(),
                scratch.clone(),
                operation.clone(),
                send.clone(),
                until,
            )
        })
        .collect::<Vec<_>>();
    drop(send);
    let mut ready = true;
    for reader in &readers {
        ready &= reader
            .ready
            .recv_timeout(until.saturating_duration_since(Instant::now()))
            .is_ok();
    }
    for reader in &readers {
        let _ = reader.go.send(());
    }
    let results = (0..readers.len())
        .map(|_| receive.recv_timeout(until.saturating_duration_since(Instant::now())))
        .collect::<Vec<_>>();
    while !readers.iter().all(|reader| reader.worker.is_finished()) && Instant::now() < until {
        thread::yield_now();
    }
    if !readers.iter().all(|reader| reader.worker.is_finished()) {
        eprintln!(
            "recovery reader outlived the shared completion deadline; aborting before fixture guards unwind"
        );
        std::process::abort();
    }
    for reader in readers {
        reader.worker.join().unwrap();
    }
    assert!(ready, "both readers reached the bounded start gate");
    let results = results.into_iter().map(Result::unwrap).collect::<Vec<_>>();
    assert!(results.iter().any(Result::is_ok));
    assert!(results.iter().all(|result| result.is_ok()
        || matches!(result, Err(hf_application::recovery::RecoveryError::Busy))));
    assert_eq!(before, source_hashes(&root));
}

struct Reader {
    ready: mpsc::Receiver<()>,
    go: mpsc::SyncSender<()>,
    worker: thread::JoinHandle<()>,
}

fn spawn_reader(
    root: PathBuf,
    scratch: PathBuf,
    operation: Id,
    send: mpsc::Sender<
        Result<
            Option<hf_domain::recovery::RecoveryRecord>,
            hf_application::recovery::RecoveryError,
        >,
    >,
    until: Instant,
) -> Reader {
    let (ready_send, ready) = mpsc::sync_channel(1);
    let (go, go_receive) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let session = RecoveryReadSession::new(
            root,
            scratch,
            Limits::default(),
            crate::tests::lease::lease_port(),
        );
        ready_send.send(()).unwrap();
        let started = go_receive
            .recv_timeout(until.saturating_duration_since(Instant::now()))
            .is_ok();
        let result = if started {
            session.read(&operation, until)
        } else {
            Err(hf_application::recovery::RecoveryError::DeadlineExceeded)
        };
        send.send(result).unwrap();
    });
    Reader { ready, go, worker }
}
