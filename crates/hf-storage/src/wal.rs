//! Until a shared schema-qualification protocol exists, initial open/probe only
//! accept quiescent checkpointed stores. They never checkpoint another connection.
use crate::{Error, Result};
use std::{fs, path::Path};
pub(crate) fn qualify(database: &Path) -> Result<()> {
    let path = database.with_file_name("store.sqlite3-wal");
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(Error::InvalidRoot)
        }
        Ok(metadata) if metadata.len() > 0 => Err(Error::NeedsCheckpoint),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
