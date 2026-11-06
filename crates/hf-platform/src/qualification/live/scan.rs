use super::super::FixtureRoot;
use std::{
    fs::{self, Metadata, OpenOptions},
    io::{self, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::PathBuf,
    time::Instant,
};
#[derive(Eq, PartialEq)]
struct Stamp {
    dev: u64,
    ino: u64,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
    mode: u32,
    uid: u32,
}
impl Stamp {
    fn of(m: &Metadata) -> Self {
        Self {
            dev: m.dev(),
            ino: m.ino(),
            size: m.len(),
            modified: (m.mtime(), m.mtime_nsec()),
            changed: (m.ctime(), m.ctime_nsec()),
            mode: m.mode(),
            uid: m.uid(),
        }
    }
}
/// Contains original owned file identities, never the needle or file bytes.
pub struct SecretMatches {
    root: PathBuf,
    matches: Vec<(PathBuf, Stamp)>,
}
impl SecretMatches {
    pub fn count(&self) -> usize {
        self.matches.len()
    }
    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }
}
/// Caller attests positive original waits, capture EOF, helper/proxy joins and owned
/// socket closure for every actor. Unknown actors prohibit construction and removal.
pub struct OriginalActorsSettled {
    _private: (),
}
impl OriginalActorsSettled {
    /// This is caller evidence attestation, not an independent containment proof.
    pub fn attest_original_waits_eof_and_joins() -> Self {
        Self { _private: () }
    }
}
/// Only a constructor-owned root grants scan authority. Does not log or retain needle.
/// Filesystem calls cannot be preempted. Call only after original actors settle.
pub fn scan_secret(root: &FixtureRoot, needle: &[u8], until: Instant) -> io::Result<SecretMatches> {
    if needle.is_empty() || needle.len() > 4096 {
        return Err(io::Error::other("secret scan needle budget"));
    }
    bounded(until, || root.validate_topology())?;
    let mut result = SecretMatches {
        root: root.root().to_owned(),
        matches: Vec::new(),
    };
    let mut remaining = 1024;
    let mut bytes = 32 * 1024 * 1024;
    walk(
        root.root().to_owned(),
        needle,
        &mut remaining,
        &mut bytes,
        &mut result,
        0,
        until,
    )?;
    bounded(until, || root.validate_topology())?;
    Ok(result)
}
fn walk(
    path: PathBuf,
    needle: &[u8],
    remaining: &mut usize,
    bytes: &mut u64,
    result: &mut SecretMatches,
    depth: usize,
    until: Instant,
) -> io::Result<()> {
    if *remaining == 0 || depth > 16 {
        return Err(io::Error::other("secret scan entry budget"));
    }
    *remaining -= 1;
    let meta = bounded(until, || fs::symlink_metadata(&path))?;
    let stamp = Stamp::of(&meta);
    if meta.file_type().is_symlink()
        || meta.uid() != bounded(until, || fs::metadata(&result.root))?.uid()
        || meta.mode() & 0o022 != 0
    {
        return Err(io::Error::other("secret scan topology refused"));
    }
    if meta.is_dir() {
        let mut entries = bounded(until, || fs::read_dir(&path))?;
        loop {
            let entry = bounded(until, || entries.next().transpose())?;
            let Some(entry) = entry else {
                break;
            };
            walk(
                entry.path(),
                needle,
                remaining,
                bytes,
                result,
                depth + 1,
                until,
            )?;
        }
    } else if meta.is_file() && meta.nlink() == 1 && meta.len() <= *bytes {
        *bytes -= meta.len();
        if contains(&path, &stamp, needle, until)? {
            result.matches.push((
                path.clone(),
                Stamp::of(&bounded(until, || fs::symlink_metadata(&path))?),
            ));
        }
    } else {
        return Err(io::Error::other("secret scan file budget"));
    }
    if Stamp::of(&bounded(until, || fs::symlink_metadata(&path))?) != stamp {
        return Err(io::Error::other("secret scan identity changed"));
    }
    Ok(())
}
fn contains(path: &PathBuf, stamp: &Stamp, needle: &[u8], until: Instant) -> io::Result<bool> {
    let mut file = bounded(until, || {
        OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(path)
    })?;
    if Stamp::of(&bounded(until, || file.metadata())?) != *stamp {
        return Err(io::Error::other("secret scan open identity"));
    }
    let mut tail = Vec::new();
    let mut buffer = [0; 16384];
    let mut read = 0_u64;
    let mut found = false;
    loop {
        let n = bounded(until, || file.read(&mut buffer))?;
        if n == 0 {
            break;
        }
        read += n as u64;
        if read > stamp.size {
            return Err(io::Error::other("secret scan growth"));
        }
        tail.extend_from_slice(&buffer[..n]);
        found |= tail.windows(needle.len()).any(|window| window == needle);
        let keep = (needle.len() - 1).min(tail.len());
        tail.drain(..tail.len() - keep);
    }
    if read != stamp.size || Stamp::of(&bounded(until, || file.metadata())?) != *stamp {
        return Err(io::Error::other("secret scan read identity"));
    }
    Ok(found)
}
/// Removes only exact original matches after caller's positive settlement evidence.
/// No secure erase claim. Other fixture evidence is retained. No arbitrary paths.
pub fn remove_matched_files(
    root: &FixtureRoot,
    matches: SecretMatches,
    _settled: &OriginalActorsSettled,
    until: Instant,
) -> io::Result<usize> {
    bounded(until, || root.validate_topology())?;
    if matches.root != root.root() {
        return Err(io::Error::other("secret removal root mismatch"));
    }
    let mut removed = 0;
    for (path, stamp) in matches.matches {
        bounded(until, || root.validate_topology())?;
        if !path.starts_with(root.root())
            || Stamp::of(&bounded(until, || fs::symlink_metadata(&path))?) != stamp
        {
            return Err(io::Error::other("secret removal identity changed"));
        }
        bounded(until, || fs::remove_file(path))?;
        removed += 1;
    }
    bounded(until, || root.validate_topology())?;
    Ok(removed)
}

/// Calls are nonpreemptible; expiry never admits the next filesystem operation.
fn bounded<T>(until: Instant, operation: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    check_deadline(until)?;
    let result = operation();
    check_deadline(until)?;
    result
}
fn check_deadline(until: Instant) -> io::Result<()> {
    if Instant::now() >= until {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "secret scan deadline",
        ))
    } else {
        Ok(())
    }
}
