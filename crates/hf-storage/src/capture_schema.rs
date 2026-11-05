use crate::{Error, Result};
use rusqlite::Connection;
pub(crate) const SCHEMA: &str = "
CREATE TABLE capture_meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL,generation TEXT NOT NULL);
INSERT INTO capture_meta SELECT 1,1,generation FROM store_identity WHERE singleton=1;
CREATE TABLE capture_records(operation TEXT PRIMARY KEY NOT NULL,binding TEXT NOT NULL,revision INTEGER NOT NULL,stdout INTEGER NOT NULL,stderr INTEGER NOT NULL,stdout_eof INTEGER NOT NULL,stderr_eof INTEGER NOT NULL,loss INTEGER NOT NULL,exit_kind INTEGER NOT NULL,exit_value INTEGER NOT NULL);
CREATE TABLE capture_events(operation TEXT NOT NULL,revision INTEGER NOT NULL,stdout INTEGER NOT NULL,stderr INTEGER NOT NULL,stdout_eof INTEGER NOT NULL,stderr_eof INTEGER NOT NULL,loss INTEGER NOT NULL,exit_kind INTEGER NOT NULL,exit_value INTEGER NOT NULL,PRIMARY KEY(operation,revision));
";
pub(crate) fn check(db: &Connection, generation: &str) -> Result<()> {
    let row: (Option<i64>, Option<String>, i64) = db.query_row("SELECT CASE WHEN typeof(version)='integer' THEN version END,CASE WHEN typeof(generation)='text' AND length(CAST(generation AS BLOB))=32 THEN generation END,(SELECT count(*) FROM (SELECT 1 FROM capture_meta LIMIT 2)) FROM capture_meta WHERE singleton=1", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    if row.0 != Some(1) || row.1.as_deref() != Some(generation) || row.2 != 1 {
        return Err(Error::InvalidOwnership);
    }
    Ok(())
}
