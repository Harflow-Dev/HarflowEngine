//! Store marker, rollback-journal and schema-version checks.
use crate::{Error, Identity, Result, binding::Binding};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
pub(crate) fn marker(binding: &Binding) -> Result<Identity> {
    read_marker(binding, false)
}
/// As `marker`, for the read session: a non-UTF-8 marker is `Incompatible` and an
/// older schema is `UnsupportedLayout`, as the lease-free inspector reports them.
pub(crate) fn marker_distinct(binding: &Binding) -> Result<Identity> {
    read_marker(binding, true)
}
fn read_marker(binding: &Binding, distinct: bool) -> Result<Identity> {
    read_marker_at(binding.root(), distinct)
}
pub(crate) fn read_marker_at(root: &Path, distinct: bool) -> Result<Identity> {
    let file = File::open(root.join("store.identity"))?;
    if file.metadata()?.len() > 128 {
        return Err(Error::Incompatible);
    }
    let mut text = String::new();
    match file.take(129).read_to_string(&mut text) {
        Err(error) if distinct && error.kind() == std::io::ErrorKind::InvalidData => {
            return Err(Error::Incompatible);
        }
        other => {
            other?;
        }
    }
    let mut lines = text.lines();
    if lines.next() != Some("harflow-storage-2") {
        return Err(Error::Incompatible);
    }
    let version = lines
        .next()
        .ok_or(Error::Incompatible)?
        .parse::<u32>()
        .map_err(|_| Error::Incompatible)?;
    // The inspector validates the whole marker before it compares schemas, so the
    // distinct path does too; `Store`'s path keeps its order.
    if !distinct {
        classify_schema(version, false)?;
    }
    let generation = lines.next().ok_or(Error::Incompatible)?.to_owned();
    if generation.len() != 32
        || !generation
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || lines.next().is_some()
        || text != format!("harflow-storage-2\n{version}\n{generation}\n")
    {
        return Err(Error::Incompatible);
    }
    if distinct {
        classify_schema(version, true)?;
    }
    Ok(Identity {
        generation,
        schema_version: version,
    })
}
fn classify_schema(version: u32, distinct: bool) -> Result<()> {
    if version > crate::SCHEMA_VERSION {
        return Err(Error::FutureSchema);
    }
    if version != crate::SCHEMA_VERSION {
        return Err(if distinct {
            Error::UnsupportedLayout
        } else {
            Error::Incompatible
        });
    }
    Ok(())
}
pub(crate) fn publish(root: &Path, identity: &Identity) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("store.identity"))?;
    write!(
        file,
        "harflow-storage-2\n{}\n{}\n",
        identity.schema_version, identity.generation
    )?;
    file.sync_all()?;
    Ok(())
}
/// A nonempty rollback journal is outside this WAL-only protocol.
pub(crate) fn reject_nonempty_journal(binding: &Binding) -> Result<()> {
    let journal = binding.root().join("store.sqlite3-journal");
    match fs::metadata(journal) {
        Ok(metadata) if metadata.len() != 0 => Err(Error::NeedsCheckpoint),
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}
