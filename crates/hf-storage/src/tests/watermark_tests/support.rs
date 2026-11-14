#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! A writer child stopped at a watermark stage hook, a recording flush port and file
//! helpers for the stage-hook tests of the commit watermark.
use crate::{Limits, Mutation, Store, WriterOptions};
use hf_application::{
    durability::FlushPort,
    watermark::{self, Decoded, FILE_LEN, NAME, Slot},
};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    os::unix::fs::FileExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};

pub(super) fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

pub(super) struct Scratch(pub PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// An initialized root after `commits` commits, with no handle left open.
pub(super) fn root(name: &str, commits: u32) -> (Scratch, PathBuf) {
    let parent = std::env::temp_dir().join(format!("hf-storage-wm-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let scratch = Scratch(fs::canonicalize(parent).unwrap());
    let root = scratch.0.join("store");
    let leases = crate::tests::lease::lease_port();
    let store = Store::initialize_fixture(&root, Limits::default(), deadline(), leases).unwrap();
    for number in 0..commits {
        put(&store, &format!("key-{number}")).unwrap();
    }
    drop(store);
    (scratch, root)
}

/// One commit. `io.lock` never waits, and a concurrent spawn by another test can briefly
/// hold a duplicate of a released lock, so `Busy` is retried every 5 ms, as callers do.
pub(super) fn put(store: &Store, key: &str) -> crate::Result<()> {
    let mutation = Mutation::Put {
        key: key.into(),
        value: key.as_bytes().to_vec(),
    };
    for _ in 0..100 {
        match store.apply_batch(std::slice::from_ref(&mutation), deadline()) {
            Err(crate::Error::Busy) => std::thread::sleep(Duration::from_millis(5)),
            other => return other,
        }
    }
    Err(crate::Error::Busy)
}

pub(super) fn open(root: &Path, options: WriterOptions) -> Store {
    let leases = crate::tests::lease::lease_port();
    Store::open_with(root, Limits::default(), deadline(), leases, options).unwrap()
}

pub(super) fn session(root: &Path) -> crate::Result<crate::ReadSession> {
    let leases = crate::tests::lease::lease_port();
    crate::ReadSession::open(root, Limits::default(), deadline(), leases)
}

pub(super) fn generation(root: &Path) -> String {
    let marker = fs::read_to_string(root.join("store.identity")).unwrap();
    marker.lines().nth(2).unwrap().to_owned()
}

pub(super) fn decoded_bytes(bytes: &[u8], generation: &str) -> Decoded {
    watermark::decode(bytes, generation)
}

pub(super) fn decoded(root: &Path) -> Decoded {
    decoded_bytes(&fs::read(root.join(NAME)).unwrap(), &generation(root))
}

pub(super) fn database_seq(root: &Path) -> u64 {
    let connection = rusqlite::Connection::open_with_flags(
        root.join("store.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let value: i64 = connection
        .query_row(
            "SELECT value FROM commit_sequence WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    u64::try_from(value).unwrap()
}

/// A whole file whose blocks hold `a` and `b` (each `None` is all zero).
pub(super) fn file_of(generation: &str, a: Option<u64>, b: Option<u64>) -> Vec<u8> {
    let mut bytes = vec![0u8; FILE_LEN];
    for (slot, seq) in [(Slot::A, a), (Slot::B, b)] {
        if let Some(seq) = seq {
            let start = usize::try_from(slot.offset()).unwrap();
            let payload = watermark::encode_payload(generation, seq).unwrap();
            bytes[start..start + payload.len()].copy_from_slice(&payload);
        }
    }
    bytes
}

/// Overwrites the start of `slot` with the start of a newer payload: a torn write.
pub(super) fn tear(root: &Path, slot: Slot) {
    let newer = watermark::encode_payload(&generation(root), u64::MAX / 2).unwrap();
    let file = OpenOptions::new()
        .write(true)
        .open(root.join(NAME))
        .unwrap();
    file.write_all_at(&newer[..44], slot.offset()).unwrap();
}

/// Keeps the file image at every flush, in order.
#[derive(Debug, Default)]
pub(super) struct Recorder {
    pub images: Mutex<Vec<Vec<u8>>>,
}
impl FlushPort for Recorder {
    fn flush(&self, file: &File) -> std::io::Result<()> {
        let mut image = vec![0u8; FILE_LEN];
        let read = file.read_at(&mut image, 0)?;
        image.truncate(read);
        self.images.lock().unwrap().push(image);
        Ok(())
    }
}

/// A writer child of this test binary, stopped at a stage hook until resumed or killed.
pub(super) struct Staged {
    child: super::super::StageChild,
    lines: mpsc::Receiver<Option<String>>,
}

impl Staged {
    /// Runs `watermark_tests::watermark_stage_child` on `root` in `mode` and waits until
    /// it reports `stage`.
    pub(super) fn start(root: &Path, mode: &str, stage: &str) -> Self {
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::watermark_tests::watermark_stage_child",
                "--nocapture",
            ])
            .env("HF_WATERMARK_ROOT", root)
            .env("HF_WATERMARK_MODE", mode)
            .env("HF_WATERMARK_STAGE", stage)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut child = super::super::StageChild(child);
        let output = child.0.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if sender.send(line.ok()).is_err() {
                    return;
                }
            }
            let _ = sender.send(None);
        });
        let mut staged = Self { child, lines };
        staged.expect(&format!("STAGE:{stage}"));
        staged
    }
    /// The first line starting with `prefix`, without it.
    pub(super) fn expect(&mut self, prefix: &str) -> String {
        for _ in 0..64 {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(10))
                .expect("stage child output timeout")
                .expect("stage child exited early");
            if let Some(rest) = line.trim().strip_prefix(prefix) {
                return rest.to_owned();
            }
        }
        panic!("{prefix} was not printed");
    }
    pub(super) fn resume(&mut self) {
        let input = self.child.0.stdin.as_mut().unwrap();
        input.write_all(b"\n").unwrap();
        input.flush().unwrap();
    }
    pub(super) fn finish(mut self) {
        drop(self.child.0.stdin.take());
        assert!(self.child.0.wait().unwrap().success());
    }
    /// Kills the child while its stdin stays open: `Child::wait` closes stdin first, and
    /// the end of input would release the stage hook while the kill is still landing.
    pub(super) fn kill(mut self) {
        let input = self.child.0.stdin.take();
        self.child.0.kill().unwrap();
        assert!(!self.child.0.wait().unwrap().success());
        drop(input);
    }
}
