//! Event-log and prompt-blob operations.
use crate::fail::Result;
use hf_domain::task_read::TaskId;
use hf_interfaces::fixture_port::{EventSpec, Response};
use hf_storage::{FixtureLine, Store};
use std::time::{Duration, Instant};
/// `not json` and a newline.
const MALFORMED: u64 = 9;
/// `{"partial":` without a newline.
const PARTIAL: u64 = 11;
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(30)
}
pub fn append_event(store: &Store, id: &TaskId, event: &EventSpec) -> Result<Response> {
    let line = FixtureLine::Event {
        seq: event.seq,
        kind: event.kind.clone(),
        body: event.body(id),
    };
    store.append_event_record_fixture(id, &line, deadline())?;
    Ok(Response::Null)
}
pub fn append_malformed(store: &Store, id: &TaskId) -> Result<Response> {
    store.append_event_record_fixture(
        id,
        &FixtureLine::Malformed { len: MALFORMED },
        deadline(),
    )?;
    Ok(Response::Null)
}
pub fn append_partial(store: &Store, id: &TaskId) -> Result<Response> {
    store.append_event_record_fixture(id, &FixtureLine::Partial { len: PARTIAL }, deadline())?;
    Ok(Response::Null)
}
pub fn truncate(store: &Store, id: &TaskId, keep: u64) -> Result<Response> {
    store.truncate_event_log_fixture(id, keep, deadline())?;
    Ok(Response::Null)
}
pub fn remove(store: &Store, id: &TaskId) -> Result<Response> {
    store.remove_event_log_fixture(id, deadline())?;
    Ok(Response::Null)
}
/// The stored full prompt no longer matches its claimed digest; the digest is untouched.
pub fn tamper_prompt(store: &Store, id: &TaskId) -> Result<Response> {
    store.append_prompt_blob_fixture(id, b"tampered", deadline())?;
    Ok(Response::Null)
}
