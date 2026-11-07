//! The authoritative digest: raw bytes of the root, never a database open.
//!
//! SHA-256 over, in sorted relative-path order, each entry's path, type and length followed
//! by the file bytes or the symlink target. `store.lock`, `io.lock` and the shared-memory
//! index are excluded by name; `-wal` is included, since with checkpoints suppressed it
//! holds the seeded history.
use crate::{
    fail::{Fail, Result},
    root::Context,
};
use hf_interfaces::fixture_port::Response;
use hf_storage::Sha256Stream;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
const EXCLUDED: [&str; 3] = ["store.lock", "io.lock", "store.sqlite3-shm"];
const LIMIT: usize = 256 * 1024 * 1024;
pub fn authoritative_digest(context: &Context) -> Result<Response> {
    let metadata = std::fs::symlink_metadata(&context.root)?;
    if !metadata.is_dir() {
        return Err(Fail::new("the root is not a directory"));
    }
    let mut entries = Vec::new();
    collect(&context.root, Path::new(""), &mut entries)?;
    entries.sort();
    let mut hash = Sha256Stream::with_limit(LIMIT).map_err(|_| Fail::new("digest limit"))?;
    for relative in &entries {
        entry(&mut hash, &context.root, relative)?;
    }
    let digest = hash
        .finish()
        .map_err(|_| Fail::new("the root exceeds the digest limit"))?;
    Ok(Response::Text(digest.as_str().to_owned()))
}
fn collect(root: &Path, relative: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for item in std::fs::read_dir(root.join(relative))? {
        let name = item?.file_name();
        if EXCLUDED.iter().any(|excluded| name == *excluded) {
            continue;
        }
        let path = relative.join(&name);
        let kind = std::fs::symlink_metadata(root.join(&path))?.file_type();
        out.push(path.clone());
        if kind.is_dir() {
            collect(root, &path, out)?;
        }
    }
    Ok(())
}
fn put(hash: &mut Sha256Stream, bytes: &[u8]) -> Result<()> {
    hash.update(bytes)
        .map_err(|_| Fail::new("the root exceeds the digest limit"))
}
fn entry(hash: &mut Sha256Stream, root: &Path, relative: &Path) -> Result<()> {
    let path = root.join(relative);
    let metadata = std::fs::symlink_metadata(&path)?;
    let kind = metadata.file_type();
    let name = raw(relative.as_os_str())?;
    put(hash, &(name.len() as u64).to_be_bytes())?;
    put(hash, &name)?;
    if kind.is_dir() {
        put(hash, b"d")?;
        return put(hash, &0u64.to_be_bytes());
    }
    if kind.is_symlink() {
        let target = raw(std::fs::read_link(&path)?.as_os_str())?;
        put(hash, b"l")?;
        put(hash, &(target.len() as u64).to_be_bytes())?;
        return put(hash, &target);
    }
    if !kind.is_file() {
        return Err(Fail::new(format!(
            "unsupported file type: {}",
            relative.display()
        )));
    }
    put(hash, b"f")?;
    put(hash, &metadata.len().to_be_bytes())?;
    let mut file = File::open(&path)?;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
        put(hash, &buffer[..count])?;
    }
}
/// The exact bytes of a name, so distinct non-UTF-8 names never hash alike.
#[cfg(unix)]
fn raw(name: &std::ffi::OsStr) -> Result<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    Ok(name.as_bytes().to_vec())
}
#[cfg(not(unix))]
fn raw(_name: &std::ffi::OsStr) -> Result<Vec<u8>> {
    Err(Fail::new("the digest reads raw names on unix only"))
}
