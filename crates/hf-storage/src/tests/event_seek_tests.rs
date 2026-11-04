use super::Limits;
use crate::{Store, event_scan};
use std::{
    fs,
    time::{Duration, Instant},
};

#[test]
fn production_seek_query_caps_runtime_kind_projection_and_avoids_body_materialization() {
    let _guard = super::fixture_guard();
    let parent = std::env::temp_dir().join(format!("hf-event-projection-{}", std::process::id()));
    let _ = fs::remove_dir_all(&parent);
    fs::create_dir(&parent).unwrap();
    let parent = fs::canonicalize(parent).unwrap();
    let root = parent.join("store");
    let store = Store::initialize_fixture(
        &root,
        Limits::default(),
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    let db = rusqlite::Connection::open(root.join("store.sqlite3")).unwrap();
    let long_kind = "k".repeat(500_000);
    db.execute("INSERT INTO event_records(task,start,end,class,seq,kind,body) VALUES(?1,0,1,0,1,?2,zeroblob(500000))",rusqlite::params!["oversized-kind",long_kind]).unwrap();
    let value = db
        .query_row(
            &event_scan::seek_sql(false),
            rusqlite::params!["oversized-kind", 0i64, 1i64, 0i64],
            |row| {
                Ok((
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(value.0.len(), 129);
    assert_eq!(value.1, 500_000);
    assert_eq!(value.2, "blob");
    drop(db);
    drop(store);
    fs::remove_dir_all(parent).unwrap();
}
