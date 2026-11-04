use crate::{APPLICATION_ID, Error, Identity, Result, SCHEMA_VERSION};
use rusqlite::Connection;
use std::{
    fs::File,
    io::Read,
    path::Path,
    time::{Duration, Instant},
};
pub(crate) fn check_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        Err(Error::DeadlineExceeded)
    } else {
        Ok(())
    }
}
pub(crate) fn timeout(connection: &Connection, deadline: Instant) -> Result<()> {
    check_deadline(deadline)?;
    connection.busy_timeout(
        deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_secs(30)),
    )?;
    Ok(())
}
pub(crate) fn qualify_header(path: &Path) -> Result<()> {
    let mut header = [0; 100];
    File::open(path)?
        .read_exact(&mut header)
        .map_err(|_| Error::Incompatible)?;
    qualify_page(&header)
}
pub(crate) fn qualify_page(header: &[u8]) -> Result<()> {
    if &header[..16] != b"SQLite format 3\0" {
        return Err(Error::Incompatible);
    }
    let version = u32::from_be_bytes([header[60], header[61], header[62], header[63]]);
    let application = u32::from_be_bytes([header[68], header[69], header[70], header[71]]);
    if application != APPLICATION_ID {
        return Err(Error::Incompatible);
    }
    if version > SCHEMA_VERSION {
        return Err(Error::FutureSchema);
    }
    if version != SCHEMA_VERSION {
        return Err(Error::Incompatible);
    }
    Ok(())
}
pub(crate) fn identity(connection: &Connection, deadline: Instant) -> Result<Identity> {
    timeout(connection, deadline)?;
    let application: u32 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    timeout(connection, deadline)?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if application != APPLICATION_ID {
        return Err(Error::Incompatible);
    }
    if version > SCHEMA_VERSION {
        return Err(Error::FutureSchema);
    }
    if version != SCHEMA_VERSION {
        return Err(Error::Incompatible);
    }
    timeout(connection, deadline)?;
    let generation: String = connection.query_row(
        "SELECT substr(generation,1,33) FROM store_identity WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    if generation.len() != 32 || !generation.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Incompatible);
    }
    Ok(Identity {
        generation,
        schema_version: version,
    })
}
pub(crate) fn suppress_checkpoints(connection: &Connection) -> Result<()> {
    use rusqlite::config::DbConfig;
    let option = DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE;
    connection.set_db_config(option, true)?;
    if !connection.db_config(option)? {
        return Err(Error::Incompatible);
    }
    connection.pragma_update(None, "wal_autocheckpoint", 0)?;
    let value: u32 = connection.pragma_query_value(None, "wal_autocheckpoint", |row| row.get(0))?;
    if value != 0 {
        return Err(Error::Incompatible);
    }
    Ok(())
}
pub(crate) fn configure(connection: &Connection, deadline: Instant) -> Result<()> {
    timeout(connection, deadline)?;
    let mode: String = connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    timeout(connection, deadline)?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    timeout(connection, deadline)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    timeout(connection, deadline)?;
    let sync: u32 = connection.pragma_query_value(None, "synchronous", |row| row.get(0))?;
    timeout(connection, deadline)?;
    let foreign: u32 = connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    if mode != "wal" || sync != 2 || foreign != 1 {
        return Err(Error::Incompatible);
    }
    check_deadline(deadline)
}
pub(crate) fn initialize(connection: &mut Connection, deadline: Instant) -> Result<()> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(&format!("PRAGMA application_id={APPLICATION_ID}; PRAGMA user_version={SCHEMA_VERSION};
        CREATE TABLE store_identity(singleton INTEGER PRIMARY KEY CHECK(singleton=1),generation TEXT NOT NULL);
        INSERT INTO store_identity VALUES(1,lower(hex(randomblob(16))));
        CREATE TABLE fixture_records(key TEXT PRIMARY KEY NOT NULL,value BLOB NOT NULL);"))?;
    transaction.execute_batch(crate::task_fixture::TASK_SCHEMA)?;
    timeout(&transaction, deadline)?;
    transaction
        .commit()
        .map_err(|_| Error::CommitOutcomeUnknown)?;
    // Publish the fixed schema in the main file before allowing header qualification.
    timeout(connection, deadline)?;
    let (busy, log, done): (u32, i64, i64) =
        connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
    if busy != 0 || log != done {
        return Err(Error::Busy);
    }
    Ok(())
}

pub(crate) fn claim(file: &File, shared: bool) -> Result<()> {
    let result = if shared {
        file.try_lock_shared()
    } else {
        file.try_lock()
    };
    result.map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => Error::Busy,
        std::fs::TryLockError::Error(error) => Error::Io(error),
    })
}
