//! Lost-WAL acceptance record: `store.wal-acceptance` in the state root.
//!
//! Content: `harflow-wal-acceptance-1\n<generation>\n<pending|accepted|failed>\n`,
//! at most 128 bytes. Only the maintenance actor writes it (E2-B); every opener and
//! reader checks it after `-wal`. Absent means no acceptance is in progress; only
//! `accepted` for the current generation lets the root be used.
use crate::{Error, Result};
use std::{fs, io::Read, path::Path};

const NAME: &str = "store.wal-acceptance";
const HEADER: &str = "harflow-wal-acceptance-1";
const BOUND: u64 = 128;

/// Refuses a root whose acceptance record is not `accepted` for `generation`.
/// `pending` and `failed` read as a missing `-wal`; a foreign generation, malformed
/// or oversized content, a non-regular file, or a file that is not the one `lstat` listed (device/inode
/// mismatch after open) is `Incompatible`. It never creates
/// or writes a file.
pub(crate) fn check(root: &Path, generation: &str) -> Result<()> {
    let path = root.join(NAME);
    let listed = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
        Ok(metadata) if !metadata.is_file() || metadata.len() > BOUND => {
            return Err(Error::Incompatible);
        }
        Ok(metadata) => metadata,
    };
    let file = fs::File::open(&path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || !same_file(&listed, &opened) {
        return Err(Error::Incompatible);
    }
    let mut bytes = Vec::new();
    file.take(BOUND + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > BOUND {
        return Err(Error::Incompatible);
    }
    let text = String::from_utf8(bytes).map_err(|_| Error::Incompatible)?;
    let state = text
        .strip_prefix(HEADER)
        .and_then(|rest| rest.strip_prefix('\n'))
        .and_then(|rest| rest.strip_prefix(generation))
        .and_then(|rest| rest.strip_prefix('\n'))
        .and_then(|rest| rest.strip_suffix('\n'))
        .ok_or(Error::Incompatible)?;
    match state {
        "accepted" => Ok(()),
        "pending" | "failed" => Err(Error::WalMissing),
        _ => Err(Error::Incompatible),
    }
}

/// The opened file must be the one `lstat` listed: a swap between the two calls is refused.
#[cfg(unix)]
fn same_file(listed: &fs::Metadata, opened: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    listed.dev() == opened.dev() && listed.ino() == opened.ino()
}

#[cfg(not(unix))]
fn same_file(_: &fs::Metadata, _: &fs::Metadata) -> bool {
    true
}
