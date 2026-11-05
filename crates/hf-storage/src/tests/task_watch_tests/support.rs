#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use crate::tests::fixture_guard;
use crate::{Limits, Store, TaskFixture};
use hf_application::{
    task_read::TaskRow,
    task_watch::{WatchCodecPort, WatchControl, WatchError},
};
use hf_domain::{
    task_read::{Coverage, StoredObservation, TaskId},
    task_watch::{WatchCursor, WatchReason, WatchRequest, WatchResult, WatchState},
};
use rusqlite::{Connection, OpenFlags};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
pub(super) struct Fixture(pub(super) PathBuf, std::sync::MutexGuard<'static, ()>);
impl Fixture {
    pub(super) fn new(label: &str) -> Self {
        let guard = fixture_guard();
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-watch-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap(), guard)
    }
    pub(super) fn root(&self) -> PathBuf {
        self.0.join("store")
    }
    pub(super) fn initialize(&self) -> Store {
        Store::initialize_fixture(self.root(), Limits::default(), deadline()).unwrap()
    }
    pub(super) fn open(&self) -> Store {
        Store::open(self.root(), Limits::default(), deadline()).unwrap()
    }
    pub(super) fn raw(&self) -> Connection {
        Connection::open_with_flags(
            self.root().join("store.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
        let _ = &self.1;
    }
}
pub(super) fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(4)
}
pub(super) fn row(id: &str, bytes: &[u8]) -> TaskRow {
    TaskRow {
        id: TaskId::new(id.to_owned()).unwrap(),
        stamp: 1,
        session: None,
        observation: StoredObservation {
            recorded_status: None,
            active: None,
            exited: None,
            drained: None,
            owned: None,
        },
        header: bytes.to_vec(),
        status: None,
    }
}
pub(super) fn task_fixture(rows: &[(&str, &[u8])]) -> TaskFixture {
    TaskFixture {
        rows: rows.iter().map(|(id, bytes)| row(id, bytes)).collect(),
        pending: vec![],
        coverage: Coverage {
            discovery_complete: true,
            ownership_complete: true,
            source_complete: true,
        },
    }
}
pub(super) fn publish(store: &Store, rows: &[(&str, &[u8])]) {
    store
        .replace_task_fixture(&task_fixture(rows), deadline())
        .unwrap();
}
pub(super) fn request(cursor: Option<String>, wait_seconds: u32) -> WatchRequest {
    WatchRequest {
        cursor,
        wait_seconds,
    }
}
pub(super) fn state(result: WatchResult) -> (WatchState, Option<String>, Option<WatchReason>) {
    (result.state, result.cursor, result.reason)
}

#[derive(Clone, Default)]
pub(super) struct Codec;
impl WatchCodecPort for Codec {
    fn decode(&self, raw: &str, expected_root: &str) -> Result<WatchCursor, WatchError> {
        let fields = raw.split(':').collect::<Vec<_>>();
        if fields.len() != 3 || fields[0] != expected_root {
            return Err(WatchError::InvalidCursor);
        }
        Ok(WatchCursor {
            root: fields[0].into(),
            epoch: fields[1].into(),
            revision: fields[2].parse().map_err(|_| WatchError::InvalidCursor)?,
        })
    }
    fn encode(&self, cursor: &WatchCursor) -> Result<String, WatchError> {
        Ok(format!(
            "{}:{}:{}",
            cursor.root, cursor.epoch, cursor.revision
        ))
    }
}
pub(super) struct EncodeActionCodec(pub(super) Action);
impl WatchCodecPort for EncodeActionCodec {
    fn decode(&self, _: &str, _: &str) -> Result<WatchCursor, WatchError> {
        Err(WatchError::InvalidCursor)
    }
    fn encode(&self, cursor: &WatchCursor) -> Result<String, WatchError> {
        (self.0)();
        Ok(format!(
            "{}:{}:{}",
            cursor.root, cursor.epoch, cursor.revision
        ))
    }
}
pub(super) type Action = Arc<dyn Fn() + Send + Sync>;
#[derive(Clone)]
pub(super) struct Clock {
    now: Arc<Mutex<Instant>>,
    action: Arc<Mutex<Option<Action>>>,
    cancelled: Arc<Mutex<bool>>,
    waits: Arc<Mutex<usize>>,
}
impl Default for Clock {
    fn default() -> Self {
        Self {
            now: Arc::new(Mutex::new(Instant::now())),
            action: Arc::new(Mutex::new(None)),
            cancelled: Arc::new(Mutex::new(false)),
            waits: Arc::new(Mutex::new(0)),
        }
    }
}
impl Clock {
    pub(super) fn on_wait(&self, action: impl Fn() + Send + Sync + 'static) {
        *self.action.lock().unwrap() = Some(Arc::new(action));
    }
    pub(super) fn cancel(&self) {
        *self.cancelled.lock().unwrap() = true;
    }
    pub(super) fn waits(&self) -> usize {
        *self.waits.lock().unwrap()
    }
    pub(super) fn set_now(&self, now: Instant) {
        *self.now.lock().unwrap() = now;
    }
}
impl WatchControl for Clock {
    fn now(&self) -> Instant {
        *self.now.lock().unwrap()
    }
    fn check(&self) -> Result<(), WatchError> {
        if *self.cancelled.lock().unwrap() {
            Err(WatchError::Cancelled)
        } else {
            Ok(())
        }
    }
    fn wait_until(&self, target: Instant) -> Result<(), WatchError> {
        *self.waits.lock().unwrap() += 1;
        let action = self.action.lock().unwrap().take();
        if let Some(action) = action {
            action();
        }
        *self.now.lock().unwrap() = target;
        Ok(())
    }
}
pub(super) fn watch(root: &Path, clock: Clock) -> crate::WatchSession<Codec, Clock> {
    crate::WatchSession::new(root, Limits::default(), Codec, clock)
}
pub(super) fn read_revision(db: &Connection) -> i64 {
    db.query_row(
        "SELECT revision FROM task_watch_meta WHERE singleton=1",
        [],
        |row| row.get(0),
    )
    .unwrap()
}
