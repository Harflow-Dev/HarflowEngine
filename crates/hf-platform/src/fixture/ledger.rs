pub use hf_domain::fixture::QualifiedLedger as LedgerIdentity;
use hf_domain::{ownership::Id, process::ProcessIdentity};
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    time::Instant,
};
const MAX_LEDGER: u64 = 65536;
const NAME: &str = "r03-b2.effects";
fn open(root: &Path, append: bool) -> io::Result<File> {
    let parent = root
        .parent()
        .ok_or_else(|| io::Error::other("fixture root parent"))?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .append(append)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    options.open(parent.join(NAME))
}
fn identify(file: &File) -> io::Result<LedgerIdentity> {
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > MAX_LEDGER {
        return Err(io::Error::other("fixture ledger shape"));
    }
    Ok(LedgerIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}
/// Qualifies a pre-existing test-created exclusive ledger and closes its handle.
pub fn qualify_ledger(root: &Path) -> io::Result<LedgerIdentity> {
    identify(&open(root, false)?)
}
/// One fixed append, no payload or arbitrary filename. A late filesystem result
/// never becomes a success acknowledgment. The nonblocking file lock covers
/// qualification, capacity checking, append and fsync. No execution or terminal
/// proof follows.
pub fn append_effect(
    root: &Path,
    expected: LedgerIdentity,
    operation: &Id,
    child: &ProcessIdentity,
    deadline: Instant,
) -> io::Result<()> {
    if Instant::now() >= deadline {
        return Err(io::Error::other("fixture effect deadline"));
    }
    let parent = root
        .parent()
        .ok_or_else(|| io::Error::other("fixture ledger parent"))?;
    let _lease = crate::lease::shared()
        .acquire(
            &parent.join("r03-b2.effects.lock"),
            hf_application::lease::LeaseMode::Exclusive,
            true,
            deadline,
        )
        .map_err(|_| io::Error::other("fixture ledger locked"))?;
    let mut file = open(root, true)?;
    if identify(&file)? != expected {
        return Err(io::Error::other("fixture ledger replaced"));
    }
    let record = format!(
        "{}|{}|{}|{}|{}\n",
        operation.as_str(),
        child.pid(),
        child.start_seconds(),
        child.start_microseconds(),
        child.boot_uuid()
    );
    if file.metadata()?.len() > MAX_LEDGER - record.len() as u64 {
        return Err(io::Error::other("fixture ledger capacity"));
    }
    if Instant::now() >= deadline {
        return Err(io::Error::other("fixture effect deadline"));
    }
    file.write_all(record.as_bytes())?;
    file.sync_all()?;
    if Instant::now() >= deadline {
        return Err(io::Error::other("fixture effect deadline"));
    }
    Ok(())
}
