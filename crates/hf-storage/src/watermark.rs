//! Commit watermark: `store.watermark` in the state root (format in
//! `hf_application::watermark`).
//!
//! It records the highest `commit_sequence` value whose commit was acknowledged. A
//! database whose sequence is below it has lost acknowledged commits (`Error::CommitLost`).
//! Initialization creates it at sequence 0; nothing recreates it. A missing file, or one
//! with no valid slot, is `ReadUnavailable("store.watermark")`; a wrong length, a
//! non-regular file or a foreign generation is `Incompatible`.
use crate::{Error, Result, small_file};
use hf_application::{
    durability::FlushPort,
    watermark::{self as codec, BLOCK_LEN, Decoded, FILE_LEN, NAME, Slot},
};
use rusqlite::Connection;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    time::Duration,
};

/// Extra attempts of a reader that found no valid slot, `RETRY_PAUSE` apart: a reader can
/// see one block before a commit and the other torn by the next one.
const RETRIES: u32 = 3;
const RETRY_PAUSE: Duration = Duration::from_millis(1);

fn unavailable() -> Error {
    Error::ReadUnavailable(NAME)
}

/// Creates the file at sequence 0 and flushes it. The caller syncs the directory.
pub(crate) fn create(root: &Path, generation: &str, flush: &dyn FlushPort) -> Result<()> {
    let bytes = codec::initial_file(generation).ok_or(Error::Incompatible)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(NAME))?;
    file.write_all(&bytes)?;
    flush.flush(&file)?;
    Ok(())
}

/// The best valid sequence for a reader, which holds no `io.lock`. When neither slot is
/// valid it retries the pair `RETRIES` times before reporting it unavailable.
pub(crate) fn read(root: &Path, generation: &str) -> Result<u64> {
    let file = open(root, false)?;
    for attempt in 0..=RETRIES {
        match decode(&file, generation)? {
            Decoded::Valid { seq, .. } => return Ok(seq),
            Decoded::Incompatible => return Err(Error::Incompatible),
            Decoded::Unavailable if attempt < RETRIES => std::thread::sleep(RETRY_PAUSE),
            Decoded::Unavailable => {}
        }
    }
    Err(unavailable())
}

/// `lstat` only: the file is still a regular file. Used by end-of-read re-checks, which
/// must not compare sequences (a concurrent commit legitimately moves the file ahead of
/// an older snapshot).
pub(crate) fn present(root: &Path) -> Result<()> {
    match std::fs::symlink_metadata(root.join(NAME)) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(Error::Incompatible),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(unavailable()),
        Err(error) => Err(error.into()),
    }
}

/// `Error::CommitLost` when the watermark is ahead of the database sequence. A missing
/// or malformed sequence row is `Incompatible`.
pub(crate) fn not_ahead(mark: u64, connection: &Connection) -> Result<()> {
    if mark > database_sequence(connection)? {
        return Err(Error::CommitLost);
    }
    Ok(())
}

fn database_sequence(connection: &Connection) -> Result<u64> {
    crate::write_tx::require_sequence(connection)?;
    let value: i64 = connection.query_row(
        "SELECT value FROM commit_sequence WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    u64::try_from(value).map_err(|_| Error::Incompatible)
}

/// The file opened read-write by the committing writer under `io.lock`, with its decoded
/// best slot. Only that writer changes it.
#[derive(Debug)]
pub(crate) struct Watermark {
    file: File,
    best: Slot,
    seq: u64,
}

impl Watermark {
    /// Opens and decodes the file. A missing, malformed or foreign file fails here, before
    /// any transaction, and nothing is recreated.
    pub(crate) fn open(root: &Path, generation: &str) -> Result<Self> {
        let file = open(root, true)?;
        match decode(&file, generation)? {
            Decoded::Valid { best, seq } => Ok(Self { file, best, seq }),
            Decoded::Unavailable => Err(unavailable()),
            Decoded::Incompatible => Err(Error::Incompatible),
        }
    }
    pub(crate) fn seq(&self) -> u64 {
        self.seq
    }
    /// Flushes the file as it stands, so that the best slot is durable before the other
    /// slot is overwritten (a writer that died between its write and its flush can leave
    /// a best slot that is only in the page cache).
    pub(crate) fn settle(&self, flush: &dyn FlushPort) -> std::io::Result<()> {
        flush.flush(&self.file)
    }
    /// Writes `seq` into the slot that is not the best one, then flushes. The whole
    /// 4 KiB block is written (payload and zero filler) in one positioned write, so damaged
    /// filler in the target block is repaired rather than leaving it invalid. The best
    /// block is never written, so a torn write can only damage the older entry.
    pub(crate) fn record(&self, generation: &str, seq: u64, flush: &dyn FlushPort) -> Result<()> {
        let payload = codec::encode_payload(generation, seq).ok_or(Error::Incompatible)?;
        let mut block = vec![0u8; BLOCK_LEN];
        block[..payload.len()].copy_from_slice(&payload);
        fire("watermark_before_write");
        write_at(&self.file, &block, self.best.other().offset())?;
        fire("watermark_before_flush");
        flush.flush(&self.file)?;
        Ok(())
    }
}

/// Opens the listed regular file and checks its length. Absent is unavailable.
fn open(root: &Path, write: bool) -> Result<File> {
    let (file, len) = small_file::open_listed(root, NAME, write)?.ok_or_else(unavailable)?;
    if len != FILE_LEN as u64 {
        return Err(Error::Incompatible);
    }
    Ok(file)
}

/// Reads each block with its own positioned read and decodes the pair.
fn decode(file: &File, generation: &str) -> Result<Decoded> {
    let a = read_block(file, Slot::A)?;
    let b = read_block(file, Slot::B)?;
    Ok(codec::decode_blocks(&a, &b, generation))
}

#[cfg(unix)]
fn read_block(file: &File, slot: Slot) -> std::io::Result<Vec<u8>> {
    use std::os::unix::fs::FileExt;
    let mut block = vec![0u8; BLOCK_LEN];
    file.read_exact_at(&mut block, slot.offset())?;
    Ok(block)
}

#[cfg(unix)]
fn write_at(file: &File, bytes: &[u8], offset: u64) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(bytes, offset)
}

/// Non-unix roots never reach here: `Binding` refuses them first.
#[cfg(not(unix))]
fn read_block(mut file: &File, slot: Slot) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut block = vec![0u8; BLOCK_LEN];
    file.seek(SeekFrom::Start(slot.offset()))?;
    file.read_exact(&mut block)?;
    Ok(block)
}

#[cfg(not(unix))]
fn write_at(mut file: &File, bytes: &[u8], offset: u64) -> std::io::Result<()> {
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(bytes)
}

fn fire(name: &'static str) {
    #[cfg(all(test, unix))]
    crate::tests::stage(name);
    #[cfg(not(all(test, unix)))]
    let _ = name;
}
