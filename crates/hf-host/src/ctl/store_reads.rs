//! Adapters from the task-read ports to the read-only store session and the state observer.
use hf_application::ctl::{
    state_inspect::{RootClass, StorePolicy, classify_root},
    task_reads::{ReadError, RootPort, StoreFailure, TaskSnapshotPort, TaskStorePort},
};
use hf_domain::{
    ctl::{
        continuation_stop::StopRow,
        inspect::{InspectReason, InspectStatus},
        state_root::StateRoot,
        task_rows::{BatchQuery, CallerRow, TaskRecord, TaskSummary, UnobservedRow},
    },
    task_read::{Lookup, TaskId},
};
use hf_interfaces::ctl::state_codec::JsonStateCodec;
use hf_platform::ctl::state_observer::FsStateObserver;
use hf_storage::Error;
use hf_storage::Limits;
use hf_storage::ReadRefusal;
use hf_storage::ReadSession;
use hf_storage::TaskSnapshot;
use std::time::Instant;

/// Classifies a root with the same observer and rules `state-inspect` uses.
pub struct FsRoots;

impl RootPort for FsRoots {
    fn classify(&self, root: &StateRoot) -> RootClass {
        let policy = StorePolicy {
            schema_version: hf_storage::SCHEMA_VERSION,
            application_id: hf_storage::APPLICATION_ID,
        };
        classify_root(&FsStateObserver, &JsonStateCodec, policy, root)
    }
}

fn incompatible(reason: InspectReason, message: String) -> StoreFailure {
    StoreFailure::Incompatible { reason, message }
}

fn standard(reason: InspectReason) -> StoreFailure {
    incompatible(reason, reason.message(InspectStatus::Incompatible))
}

/// Query errors are matched before the session's refusal table, whose default row would
/// otherwise swallow them.
pub(super) fn failure(error: &Error) -> StoreFailure {
    use InspectReason::{
        ForeignStore, InfrastructureUnavailable, InvalidMarker, NewerStore, UnsupportedLayout,
    };
    match error {
        Error::DeadlineExceeded | Error::Busy => return StoreFailure::Busy,
        Error::LimitExceeded
        | Error::MalformedRow
        | Error::TaskReadUnavailable
        | Error::InvalidTaskQuery
        | Error::StaleTaskCursor
        | Error::InvalidEventQuery
        | Error::EventReadUnavailable => {
            return incompatible(
                InfrastructureUnavailable,
                "Task history cannot be read".to_string(),
            );
        }
        _ => {}
    }
    match ReadRefusal::from(error) {
        ReadRefusal::Busy => StoreFailure::Busy,
        ReadRefusal::WalMissing => incompatible(
            InfrastructureUnavailable,
            "write-ahead log missing; the history cannot be trusted".to_string(),
        ),
        ReadRefusal::Unavailable { file } => {
            incompatible(InfrastructureUnavailable, format!("{file} cannot be read"))
        }
        ReadRefusal::Corrupt { message } | ReadRefusal::Other { message } => {
            incompatible(InfrastructureUnavailable, message)
        }
        ReadRefusal::InvalidMarker => standard(InvalidMarker),
        ReadRefusal::ForeignStore => standard(ForeignStore),
        ReadRefusal::UnsupportedLayout => standard(UnsupportedLayout),
        ReadRefusal::FutureSchema => standard(NewerStore),
        ReadRefusal::NonemptyJournal => standard(InfrastructureUnavailable),
        ReadRefusal::Replaced => incompatible(
            InfrastructureUnavailable,
            "the store was replaced during the read; retry".to_string(),
        ),
    }
}

pub(super) fn read<T>(result: Result<T, Error>) -> Result<T, StoreFailure> {
    result.map_err(|error| failure(&error))
}

pub(super) struct Snapshot<'a, 'b>(pub(super) &'a TaskSnapshot<'b>);

impl TaskSnapshotPort for Snapshot<'_, '_> {
    fn damaged(&self) -> Result<bool, StoreFailure> {
        read(self.0.damaged())
    }
    fn related_callers(
        &self,
        session: i64,
        ancestors: &[u32],
        limit: Option<usize>,
    ) -> Result<Vec<CallerRow>, StoreFailure> {
        read(self.0.related_callers(session, ancestors, limit))
    }
    fn lookup(&self, id: &TaskId) -> Result<Lookup, StoreFailure> {
        read(self.0.lookup(id))
    }
    fn summaries(&self, ids: &[TaskId]) -> Result<Vec<TaskSummary>, StoreFailure> {
        read(self.0.summaries(ids))
    }
    fn record(&self, id: &TaskId) -> Result<Option<TaskRecord>, StoreFailure> {
        read(self.0.record(id))
    }
    fn record_with_stop(&self, id: &TaskId) -> Result<Option<(TaskRecord, StopRow)>, StoreFailure> {
        read(self.0.record_with_stop(id))
    }
    fn batch(&self, query: &BatchQuery) -> Result<Vec<TaskRecord>, StoreFailure> {
        read(self.0.batch(query))
    }
    fn unobserved_count(&self) -> Result<u64, StoreFailure> {
        read(self.0.unobserved_count())
    }
    fn unobserved_rows(
        &self,
        after: Option<(i64, String)>,
        max_rows: usize,
    ) -> Result<Vec<UnobservedRow>, StoreFailure> {
        read(self.0.unobserved_rows(after, max_rows))
    }
    fn cursor_scope(
        &self,
        active_only: bool,
        session: Option<&str>,
        predecessor: Option<&TaskId>,
    ) -> Result<String, StoreFailure> {
        read(self.0.cursor_scope(active_only, session, predecessor))
    }
}

/// An event page can hold 200 lines, and the ctl event read keeps hidden ones in the same
/// window, so a session allows one more row than the contract's page bound. Every other
/// ctl query states its own, smaller bound.
const EVENT_PAGE_ROWS: usize = 201;

/// Opens the store read-only under a shared lease for one command.
pub struct SnapshotStore;

impl TaskStorePort for SnapshotStore {
    fn with_snapshot(
        &self,
        root: &StateRoot,
        deadline: Instant,
        action: &mut dyn FnMut(&dyn TaskSnapshotPort) -> Result<(), ReadError>,
    ) -> Result<(), ReadError> {
        let session = ReadSession::open(
            root.as_str(),
            Limits {
                max_rows: EVENT_PAGE_ROWS,
                ..Limits::default()
            },
            deadline,
            hf_platform::lease::shared(),
        )
        .map_err(|error| ReadError::Store(failure(&error)))?;
        let mut inner = Ok(());
        let outcome = session.snapshot(deadline, |snapshot| {
            inner = action(&Snapshot(snapshot));
            Ok(())
        });
        // The exit checks (a replaced store, the deadline) outrank a resolution or refusal.
        outcome.map_err(|error| ReadError::Store(failure(&error)))?;
        inner
    }
}
