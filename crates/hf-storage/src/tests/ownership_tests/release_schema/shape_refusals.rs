use super::*;

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
        let store = Store::initialize_fixture(
            &root,
            Limits::default(),
            deadline(),
            crate::tests::lease::lease_port(),
        )
        .unwrap();
        let request = request(&store, 2, 3, 4, 5, b"payload");
        store.ownership_prepare(&request, deadline()).unwrap();
        drop(store);
        let raw = crate::tests::raw_connection(root.join("store.sqlite3"));
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
        let reopened = Store::open(
            &root,
            Limits::default(),
            deadline(),
            crate::tests::lease::lease_port(),
        )
        .unwrap();
        assert!(matches!(
            reopened.ownership_observe(&request.operation, deadline()),
            Err(Error::InvalidOwnership)
        ));
        drop(reopened);
        let raw = crate::tests::raw_connection(root.join("store.sqlite3"));
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
        let store = Store::initialize_fixture(
            &root,
            Limits::default(),
            deadline(),
            crate::tests::lease::lease_port(),
        )
        .unwrap();
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
        let raw = crate::tests::raw_connection(root.join("store.sqlite3"));
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
        let reopened = Store::open(
            &root,
            Limits::default(),
            deadline(),
            crate::tests::lease::lease_port(),
        )
        .unwrap();
        assert!(matches!(
            reopened.ownership_observe(&request.operation, deadline()),
            Err(Error::InvalidOwnership)
        ));
        drop(reopened);
        let raw = crate::tests::raw_connection(root.join("store.sqlite3"));
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
