use super::*;
#[path = "release_schema/terminal_release.rs"]
mod terminal_release;

#[test]
fn raw_payload_digest_tampering_refuses_observation_before_any_grant() {
    let fixture = Fixture::new("tamper");
    let root = fixture.0.join("store");
    let store = Store::initialize_fixture(&root, Limits::default(), deadline()).unwrap();
    let request = request(&store, 2, 3, 4, 5, b"stored identity bytes");
    store.ownership_prepare(&request, deadline()).unwrap();
    drop(store);
    let raw = rusqlite::Connection::open(root.join("store.sqlite3")).unwrap();
    raw.execute(
        "UPDATE ownership_records SET payload=?1 WHERE operation=?2",
        rusqlite::params![b"tampered bytes".as_slice(), request.operation.as_str()],
    )
    .unwrap();
    drop(raw);
    let reopened = Store::open(&root, Limits::default(), deadline()).unwrap();
    assert!(matches!(
        reopened.ownership_observe(&request.operation, deadline()),
        Err(Error::InvalidOwnership)
    ));
    assert!(matches!(
        reopened.ownership_claim(
            &request.operation,
            &request.generation,
            0,
            &owner(),
            deadline()
        ),
        Err(Error::InvalidOwnership)
    ));
}

#[test]
fn schema_one_marker_refuses_before_source_sqlite_change() {
    let fixture = Fixture::new("old-schema");
    let root = fixture.0.join("store");
    let store = Store::initialize_fixture(&root, Limits::default(), deadline()).unwrap();
    let generation = store.identity().generation.clone();
    drop(store);
    let database = root.join("store.sqlite3");
    let before = fs::read(&database).unwrap();
    fs::write(
        root.join("store.identity"),
        format!("harflow-storage-2\n1\n{generation}\n"),
    )
    .unwrap();
    assert!(matches!(
        Store::open(&root, Limits::default(), deadline()),
        Err(Error::Incompatible)
    ));
    assert_eq!(fs::read(&database).unwrap(), before);
}

#[test]
fn schema_one_sqlite_header_with_current_marker_refuses_without_rewrite() {
    let fixture = Fixture::new("old-header");
    let root = fixture.0.join("store");
    let store = Store::initialize_fixture(&root, Limits::default(), deadline()).unwrap();
    drop(store);
    let database = root.join("store.sqlite3");
    let raw = rusqlite::Connection::open(&database).unwrap();
    raw.pragma_update(None, "user_version", 1).unwrap();
    drop(raw);
    let before = fs::read(&database).unwrap();
    assert!(matches!(
        Store::open(&root, Limits::default(), deadline()),
        Err(Error::Incompatible)
    ));
    assert_eq!(fs::read(&database).unwrap(), before);
}

#[test]
fn malformed_ownership_generation_projection_refuses_without_metadata_rewrite() {
    let fixture = Fixture::new("metadata-projection");
    let root = fixture.0.join("store");
    let store = Store::initialize_fixture(&root, Limits::default(), deadline()).unwrap();
    let generation = store.identity().generation.clone();
    drop(store);
    let database = root.join("store.sqlite3");
    let raw = rusqlite::Connection::open(&database).unwrap();
    let malformed = format!("{generation}\0{}", "untrusted-suffix".repeat(256));
    raw.execute("UPDATE ownership_meta SET generation=?1", [&malformed])
        .unwrap();
    let before: String = raw
        .query_row(
            "SELECT hex(CAST(generation AS BLOB)) FROM ownership_meta",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(raw);
    let reopened = Store::open(&root, Limits::default(), deadline()).unwrap();
    assert!(matches!(
        reopened.ownership_observe(&id(1), deadline()),
        Err(Error::OwnershipUnavailable)
    ));
    drop(reopened);
    let raw = rusqlite::Connection::open(&database).unwrap();
    let after: String = raw
        .query_row(
            "SELECT hex(CAST(generation AS BLOB)) FROM ownership_meta",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(after, before);
}

#[test]
fn production_preflight_sql_returns_only_scalar_bounds_for_oversized_cells() {
    let cases = [
        ("payload=zeroblob(16385)", false, 16385, 1),
        ("owner_process=printf('%0101d',1)", true, 7, 0),
        ("session=printf('%033d',1)", false, 7, 0),
        ("release_proof=printf('%02049d',1)", false, 7, 0),
    ];
    for (index, (statement, needs_claim, expected_payload, expected_valid)) in
        cases.into_iter().enumerate()
    {
        let fixture = Fixture::new(&format!("preflight-sql-{index}"));
        let root = fixture.0.join("store");
        let store = Store::initialize_fixture(&root, Limits::default(), deadline()).unwrap();
        let request = request(&store, 2, 3, 4, 5, b"payload");
        store.ownership_prepare(&request, deadline()).unwrap();
        if needs_claim {
            store
                .ownership_claim(
                    &request.operation,
                    &request.generation,
                    0,
                    &owner(),
                    deadline(),
                )
                .unwrap();
        }
        drop(store);
        let raw = rusqlite::Connection::open(root.join("store.sqlite3")).unwrap();
        raw.execute_batch(&format!("UPDATE ownership_records SET {statement}"))
            .unwrap();
        let (valid, payload_len, byte_charge): (i64, i64, i64) = raw
            .query_row(
                crate::ownership_read::PREFLIGHT_SQL,
                [request.operation.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(valid, expected_valid);
        assert_eq!(payload_len, expected_payload);
        assert!(byte_charge >= payload_len);
    }
}

#[test]
fn production_metadata_sql_returns_null_for_embedded_nul_and_oversize_generation() {
    let fixture = Fixture::new("meta-preflight-sql");
    let root = fixture.0.join("store");
    let store = Store::initialize_fixture(&root, Limits::default(), deadline()).unwrap();
    let generation = store.identity().generation.clone();
    drop(store);
    let raw = rusqlite::Connection::open(root.join("store.sqlite3")).unwrap();
    let malformed = format!("{generation}\0{}", "x".repeat(4096));
    raw.execute("UPDATE ownership_meta SET generation=?1", [&malformed])
        .unwrap();
    let (version, bounded, count): (i64, Option<String>, i64) = raw
        .query_row(crate::ownership_schema::META_SQL, [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!((version, bounded, count), (1, None, 1));
}

#[test]
fn future_root_marker_refuses_without_source_database_change() {
    let fixture = Fixture::new("future-root");
    let root = fixture.0.join("store");
    let store = Store::initialize_fixture(&root, Limits::default(), deadline()).unwrap();
    let generation = store.identity().generation.clone();
    drop(store);
    let database = root.join("store.sqlite3");
    let before = fs::read(&database).unwrap();
    fs::write(
        root.join("store.identity"),
        format!("harflow-storage-2\n99\n{generation}\n"),
    )
    .unwrap();
    assert!(matches!(
        Store::open(&root, Limits::default(), deadline()),
        Err(Error::FutureSchema)
    ));
    assert_eq!(fs::read(&database).unwrap(), before);
}

#[test]
fn future_ownership_namespace_version_refuses_without_metadata_rewrite() {
    let fixture = Fixture::new("future-namespace");
    let root = fixture.0.join("store");
    let store = Store::initialize_fixture(&root, Limits::default(), deadline()).unwrap();
    let raw = rusqlite::Connection::open(root.join("store.sqlite3")).unwrap();
    raw.execute("UPDATE ownership_meta SET version=99", [])
        .unwrap();
    let before: i64 = raw
        .query_row("SELECT version FROM ownership_meta", [], |row| row.get(0))
        .unwrap();
    drop(raw);
    let request = request(&store, 2, 3, 4, 5, b"payload");
    assert!(matches!(
        store.ownership_observe(&id(1), deadline()),
        Err(Error::OwnershipUnavailable)
    ));
    assert!(matches!(
        store.ownership_prepare(&request, deadline()),
        Err(Error::OwnershipUnavailable)
    ));
    drop(store);
    let raw = rusqlite::Connection::open(root.join("store.sqlite3")).unwrap();
    let after: i64 = raw
        .query_row("SELECT version FROM ownership_meta", [], |row| row.get(0))
        .unwrap();
    assert_eq!((before, after), (99, 99));
}

#[test]
fn missing_ownership_namespace_prepare_does_not_reinitialize_or_reset_tables() {
    let fixture = Fixture::new("missing-namespace");
    let root = fixture.0.join("store");
    let store = Store::initialize_fixture(&root, Limits::default(), deadline()).unwrap();
    let generation = store.identity().generation.clone();
    drop(store);
    let database = root.join("store.sqlite3");
    let raw = rusqlite::Connection::open(&database).unwrap();
    raw.execute_batch("DROP TABLE ownership_meta").unwrap();
    let before = table_names(&raw);
    drop(raw);
    let reopened = Store::open(&root, Limits::default(), deadline()).unwrap();
    let request = PreparedRequest {
        operation: id(2),
        session: id(3),
        reservation: id(4),
        attempt: id(5),
        generation: Id::new(generation).unwrap(),
        digest: Digest::new(expected_payload_digest(b"payload").to_owned()).unwrap(),
        payload: b"payload".to_vec(),
    };
    assert!(matches!(
        reopened.ownership_prepare(&request, deadline()),
        Err(Error::OwnershipUnavailable)
    ));
    drop(reopened);
    let raw = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(table_names(&raw), before);
    assert!(!before.iter().any(|name| name == "ownership_meta"));
}

fn table_names(connection: &rusqlite::Connection) -> Vec<String> {
    let mut statement = connection
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

#[test]
fn malformed_scalar_and_partial_nullable_groups_refuse_without_repair() {
    let corruptions = [
        ("phase", "9"),
        ("revision", "-1"),
        ("revision", "9223372036854775807"),
        ("cancelled", "2"),
        ("owner", "'00000000000000000000000000000009'"),
        ("child", "'202:12:34:abcdef01-2345-6789-abcd-ef0123456789'"),
    ];
    for (index, (column, value)) in corruptions.into_iter().enumerate() {
        let fixture = Fixture::new(&format!("bad-scalar-{index}"));
        let root = fixture.0.join("store");
        let store = Store::initialize_fixture(&root, Limits::default(), deadline()).unwrap();
        let request = request(&store, 2, 3, 4, 5, b"payload");
        store.ownership_prepare(&request, deadline()).unwrap();
        drop(store);
        let raw = rusqlite::Connection::open(root.join("store.sqlite3")).unwrap();
        raw.execute_batch(&format!("UPDATE ownership_records SET {column}={value}"))
            .unwrap();
        let before: String = raw
            .query_row(
                &format!("SELECT quote({column}) FROM ownership_records"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        drop(raw);
        let reopened = Store::open(&root, Limits::default(), deadline()).unwrap();
        assert!(matches!(
            reopened.ownership_observe(&request.operation, deadline()),
            Err(Error::InvalidOwnership)
        ));
        drop(reopened);
        let raw = rusqlite::Connection::open(root.join("store.sqlite3")).unwrap();
        let after: String = raw
            .query_row(
                &format!("SELECT quote({column}) FROM ownership_records"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(after, before);
    }
}

#[test]
fn oversized_persisted_payload_and_process_scalar_refuse_before_decoding() {
    for (index, update) in [
        ("payload", "payload=zeroblob(16385)"),
        ("owner_process", "owner_process=printf('%0101d',1)"),
    ]
    .into_iter()
    .enumerate()
    {
        let fixture = Fixture::new(&format!("oversized-cell-{index}"));
        let root = fixture.0.join("store");
        let store = Store::initialize_fixture(&root, Limits::default(), deadline()).unwrap();
        let request = request(&store, 2, 3, 4, 5, b"payload");
        store.ownership_prepare(&request, deadline()).unwrap();
        if index == 1 {
            store
                .ownership_claim(
                    &request.operation,
                    &request.generation,
                    0,
                    &owner(),
                    deadline(),
                )
                .unwrap();
        }
        drop(store);
        let raw = rusqlite::Connection::open(root.join("store.sqlite3")).unwrap();
        raw.execute_batch(&format!("UPDATE ownership_records SET {}", update.1))
            .unwrap();
        let before: String = raw
            .query_row(
                &format!(
                    "SELECT hex(CAST({} AS BLOB)) FROM ownership_records",
                    update.0
                ),
                [],
                |row| row.get(0),
            )
            .unwrap();
        drop(raw);
        let reopened = Store::open(&root, Limits::default(), deadline()).unwrap();
        assert!(matches!(
            reopened.ownership_observe(&request.operation, deadline()),
            Err(Error::InvalidOwnership)
        ));
        drop(reopened);
        let raw = rusqlite::Connection::open(root.join("store.sqlite3")).unwrap();
        let after: String = raw
            .query_row(
                &format!(
                    "SELECT hex(CAST({} AS BLOB)) FROM ownership_records",
                    update.0
                ),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(after, before);
    }
}
