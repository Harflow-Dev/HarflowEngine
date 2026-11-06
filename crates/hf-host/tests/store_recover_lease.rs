#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
//! S2 evidence with the production lease adapter: `recover` against readers.
//! Run explicitly with `--ignored`.

use hf_storage::{Error, Limits, Mutation, Store};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

// The platform lease table is process-wide and its state lock is try-locked, so
// concurrent acquisitions in one process return busy even for unrelated roots.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

fn fresh(name: &str) -> PathBuf {
    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("hf-recover-lease-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir(&base).unwrap();
    base
}

fn seeded(base: &std::path::Path) -> PathBuf {
    let root = base.join("store");
    let store = Store::initialize_fixture(
        &root,
        Limits::default(),
        deadline(),
        hf_platform::lease::shared(),
    )
    .unwrap();
    let batch = [Mutation::Put {
        key: "k".into(),
        value: vec![7; 512],
    }];
    store.apply_batch(&batch, deadline()).unwrap();
    root
}

fn read_back(root: &std::path::Path) -> Result<usize, Error> {
    let store = Store::open(
        root,
        Limits::default(),
        deadline(),
        hf_platform::lease::shared(),
    )?;
    Ok(store.read_batch(&["k".to_owned()], deadline())?.len())
}

#[test]
#[ignore]
fn read_then_recover_and_recover_then_read() {
    let _serial = SERIAL.lock().unwrap();
    let base = fresh("sequence");
    let root = seeded(&base);
    let reader = Store::open(
        &root,
        Limits::default(),
        deadline(),
        hf_platform::lease::shared(),
    )
    .unwrap();
    let refused = Store::recover(&root, deadline(), &base, hf_platform::lease::shared());
    assert!(matches!(refused, Err(Error::Busy)), "{refused:?}");
    drop(reader);
    Store::recover(&root, deadline(), &base, hf_platform::lease::shared()).unwrap();
    assert_eq!(read_back(&root).unwrap(), 1);
    let _ = fs::remove_dir_all(&base);
}

#[test]
#[ignore]
fn readers_during_repeated_recover_get_busy_or_complete_data() {
    let _serial = SERIAL.lock().unwrap();
    let base = fresh("concurrent");
    let root = seeded(&base);
    let stop = Arc::new(AtomicBool::new(false));
    let reader_root = root.clone();
    let reader_stop = stop.clone();
    let reader = std::thread::spawn(move || {
        let (mut ok, mut busy) = (0u32, 0u32);
        while !reader_stop.load(Ordering::Relaxed) {
            match read_back(&reader_root) {
                Ok(1) => ok += 1,
                Err(Error::Busy) => busy += 1,
                other => panic!("unexpected reader outcome {other:?}"),
            }
        }
        (ok, busy)
    });
    let (mut done, mut busy) = (0u32, 0u32);
    for _ in 0..200 {
        match Store::recover(&root, deadline(), &base, hf_platform::lease::shared()) {
            Ok(_) => done += 1,
            Err(Error::Busy) => busy += 1,
            other => panic!("unexpected recover outcome {other:?}"),
        }
    }
    stop.store(true, Ordering::Relaxed);
    let (ok, reader_busy) = reader.join().unwrap();
    eprintln!("recover done={done} busy={busy}; reader ok={ok} busy={reader_busy}");
    assert_eq!(read_back(&root).unwrap(), 1);
    let _ = fs::remove_dir_all(&base);
}
