use crate::{Error, Result};
use rusqlite::Connection;
pub(crate) const META_SQL: &str = "SELECT CASE WHEN typeof(version)='integer' THEN version END,CASE WHEN typeof(generation)='text' AND length(CAST(generation AS BLOB))=32 THEN generation END,(SELECT count(*) FROM (SELECT 1 FROM ownership_meta LIMIT 2)) FROM ownership_meta WHERE singleton=1";

pub(crate) const SCHEMA: &str = "
CREATE TABLE ownership_meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL,generation TEXT NOT NULL);
INSERT INTO ownership_meta SELECT 1,1,generation FROM store_identity WHERE singleton=1;
CREATE TABLE ownership_records(
 operation TEXT PRIMARY KEY NOT NULL, session TEXT NOT NULL, reservation TEXT NOT NULL UNIQUE,
 attempt TEXT NOT NULL UNIQUE,generation TEXT NOT NULL,digest TEXT NOT NULL,payload BLOB NOT NULL,
 phase INTEGER NOT NULL,revision INTEGER NOT NULL,cancelled INTEGER NOT NULL,
 owner TEXT,nonce TEXT,owner_process TEXT,child TEXT,release_proof TEXT);
CREATE UNIQUE INDEX ownership_live_session ON ownership_records(session) WHERE phase!=4;
";
pub(crate) fn check(connection: &Connection, generation: &str) -> Result<()> {
    let found: (i64, String, i64) = connection
        .query_row(META_SQL, [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(|_| Error::OwnershipUnavailable)?;
    if found.0 != 1 || found.1 != generation || found.2 != 1 {
        return Err(Error::OwnershipUnavailable);
    }
    Ok(())
}
