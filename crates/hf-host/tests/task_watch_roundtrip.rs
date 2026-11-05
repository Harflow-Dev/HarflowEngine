#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_application::{
    task_read::{Authority, TaskAuthorityPort},
    task_watch::TaskWatcher,
};
use hf_domain::{
    task_read::{AuthorizedScope, Coverage, Lookup, StoredObservation, TaskId, TaskQuery},
    task_watch::WatchRequest,
};
use hf_interfaces::{
    task_consumer::VersionLine,
    watch_codec::WatchJsonCodec,
    watch_consumer::{ConsumerState, Qualification, RefreshWork, WatchPermit, encode_result},
};
use hf_storage::{Limits, RealWatchControl, Store, TaskFixture, WatchSession};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

struct ExplicitDirectFixtureAuthority;
impl TaskAuthorityPort for ExplicitDirectFixtureAuthority {
    fn prepare(&self) -> Authority {
        Authority::Authorized(AuthorizedScope::Global)
    }
}

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-watch-roundtrip-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap().join("store"))
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        if let Some(parent) = self.0.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(3)
}
fn task_id() -> TaskId {
    TaskId::new("watch-roundtrip-task".to_owned()).unwrap()
}
fn fixture(header: &[u8]) -> TaskFixture {
    TaskFixture {
        rows: vec![hf_application::task_read::TaskRow {
            id: task_id(),
            stamp: 1,
            session: None,
            observation: StoredObservation {
                recorded_status: None,
                active: None,
                exited: None,
                drained: None,
                owned: None,
            },
            header: header.to_vec(),
            status: None,
        }],
        pending: Vec::new(),
        coverage: Coverage {
            discovery_complete: true,
            ownership_complete: true,
            source_complete: true,
        },
    }
}
fn watcher(
    root: &Path,
) -> TaskWatcher<ExplicitDirectFixtureAuthority, WatchSession<WatchJsonCodec, RealWatchControl>> {
    TaskWatcher::new(
        ExplicitDirectFixtureAuthority,
        WatchSession::new(
            root.to_path_buf(),
            Limits::default(),
            WatchJsonCodec,
            RealWatchControl::default(),
        ),
    )
}
fn request(cursor: Option<String>) -> WatchRequest {
    WatchRequest {
        cursor,
        wait_seconds: 0,
    }
}
fn synthetic_permit(root: &str) -> WatchPermit {
    // Synthetic consumer qualification only: this test does not establish an OS caller or runtime identity.
    let fixture = include_str!("../../../tests/fixtures/contracts-v1/version-info.json");
    let handshake = fixture
        .replace(
            "\"state_root\":\"/tmp/harflow-fixture-home/.harflow\"",
            &format!("\"state_root\":\"{root}\""),
        )
        .replace(
            "\"product_version\":\"0.0.10\"",
            &format!("\"product_version\":\"{}\"", env!("CARGO_PKG_VERSION")),
        );
    WatchPermit::qualify(
        handshake.as_bytes(),
        Qualification {
            state_root: root,
            os: "darwin",
            architecture: "arm64",
            version_line: VersionLine::new(0, 0).unwrap(),
        },
    )
    .unwrap()
}
fn roundtrip(root: &Path, cursor: Option<String>) -> hf_interfaces::watch_consumer::WatchDocument {
    let result = watcher(root).watch(&request(cursor), deadline()).unwrap();
    let bytes = encode_result(&result, env!("CARGO_PKG_VERSION")).unwrap();
    synthetic_permit(root.to_str().unwrap())
        .consume(&bytes)
        .unwrap()
}

#[test]
fn fixture_watch_roundtrip_invalidates_exact_reversion_and_coalesces_busy_refresh() {
    let root = Root::new();
    let initial = Store::initialize_fixture(&root.0, Limits::default(), deadline()).unwrap();
    initial
        .replace_task_fixture(&fixture(b"original"), deadline())
        .unwrap();
    drop(initial);

    let mut consumer = ConsumerState::default();
    assert_eq!(consumer.begin_wait().unwrap(), None);
    let baseline = roundtrip(&root.0, None);
    assert_eq!(baseline.state(), Some("baseline"));
    consumer.accept(&baseline).unwrap();
    assert_eq!(consumer.take_refresh(), Some(RefreshWork::Snapshot));

    // Read actual stored headers as the snapshot paired with the established cursor.
    let snapshot = Store::open(&root.0, Limits::default(), deadline()).unwrap();
    let rows = snapshot
        .task_read(
            &AuthorizedScope::Global,
            &TaskQuery::ids(vec![task_id()]).unwrap(),
            deadline(),
        )
        .unwrap();
    assert_eq!(rows.lookups, vec![Lookup::Found(task_id())]);
    assert_eq!(rows.rows[0].header, b"original");
    drop(snapshot);

    let writer = Store::open(&root.0, Limits::default(), deadline()).unwrap();
    writer
        .replace_task_fixture(&fixture(b"temporary"), deadline())
        .unwrap();
    writer
        .replace_task_fixture(&fixture(b"original"), deadline())
        .unwrap();
    drop(writer);

    consumer.begin_wait().unwrap();
    let changed = roundtrip(&root.0, consumer.cursor().map(str::to_owned));
    assert_eq!(changed.state(), Some("changed"));
    consumer.accept(&changed).unwrap();
    assert_eq!(consumer.cursor(), changed.cursor());
    assert_eq!(
        consumer.take_refresh(),
        None,
        "invalidation is retained while the snapshot is busy"
    );
    consumer.finish_refresh();
    assert_eq!(consumer.take_refresh(), Some(RefreshWork::Snapshot));
    consumer.finish_refresh();

    consumer.begin_wait().unwrap();
    let timeout = roundtrip(&root.0, consumer.cursor().map(str::to_owned));
    assert_eq!(timeout.state(), Some("timeout"));
    consumer.accept(&timeout).unwrap();
    assert_eq!(consumer.take_refresh(), Some(RefreshWork::Reconcile));
}
