//! Artifact manifests and their targets outside the state root.
use crate::{
    fail::{Fail, Result},
    root::Context,
};
use hf_domain::task_read::{AuthorizedScope, TaskId, TaskQuery};
use hf_interfaces::fixture_port::{ArtifactSpec, Response, Target, header_repo_path};
use hf_storage::{FixtureArtifact, FixturePlan, ReadSession, Store};
use std::{
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(30)
}
fn repo_path(header: Option<&[u8]>) -> Result<String> {
    header
        .and_then(header_repo_path)
        .ok_or_else(|| Fail::new("the task has no readable repo_path"))
}
fn header_of(rows: Vec<hf_application::task_read::TaskRow>) -> Option<Vec<u8>> {
    rows.into_iter().next().map(|row| row.header)
}
pub fn write_artifacts(
    store: &Store,
    id: &TaskId,
    items: &[ArtifactSpec],
    header_valid: bool,
) -> Result<Response> {
    let query = TaskQuery::StoredStatus(id.clone());
    let result = store.task_read(&AuthorizedScope::Global, &query, deadline())?;
    let repo = repo_path(header_of(result.rows).as_deref())?;
    let mut rows = Vec::new();
    let mut paths = Vec::new();
    for item in items {
        let path = item.path(&repo);
        let (device, inode, size, mtime_ns) = match &item.target {
            Target::Present => stat_present(&path)?,
            Target::Absent(o) => (o.device, o.inode, o.size, o.mtime_ns),
        };
        rows.push(FixtureArtifact {
            artifact_id: item.artifact_id.clone(),
            path: path.clone(),
            kind: item.kind.clone(),
            note: item.note.clone(),
            registered_at: item.registered_at.clone(),
            provenance_method: item.provenance_method.clone(),
            device,
            inode,
            size,
            mtime_ns,
            plan: item.plan.as_ref().map(|plan| FixturePlan {
                tool_use_id: plan.tool_use_id.clone(),
                directory: plan.directory.clone(),
            }),
        });
        paths.push(path);
    }
    store.replace_artifacts_fixture(id, header_valid, &rows, deadline())?;
    Ok(Response::Paths(paths))
}
/// Creates the target with fixed bytes and returns its real stat.
#[cfg(unix)]
fn stat_present(path: &str) -> Result<(u64, u64, u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    const BYTES: &[u8] = b"artifact bytes";
    let target = PathBuf::from(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target, BYTES)?;
    let info = std::fs::metadata(&target)?;
    let seconds = u64::try_from(info.mtime()).map_err(|_| Fail::new("negative mtime"))?;
    let nanos = u64::try_from(info.mtime_nsec()).map_err(|_| Fail::new("negative mtime"))?;
    let mtime_ns = seconds
        .checked_mul(1_000_000_000)
        .and_then(|value| value.checked_add(nanos))
        .ok_or_else(|| Fail::new("mtime out of range"))?;
    Ok((info.dev(), info.ino(), info.len(), mtime_ns))
}
#[cfg(not(unix))]
fn stat_present(_path: &str) -> Result<(u64, u64, u64, u64)> {
    Err(Fail::new("artifact stat is available on unix only"))
}
/// Appends ` changed` to the target outside the root, so its stat now differs.
pub fn change_target(context: &Context, id: &TaskId, name: &str) -> Result<Response> {
    let session = ReadSession::open(
        &context.root,
        Context::limits(),
        deadline(),
        context.leases(),
    )?;
    let query = TaskQuery::StoredStatus(id.clone());
    let result = session.task_read(&AuthorizedScope::Global, &query, deadline())?;
    let repo = repo_path(header_of(result.rows).as_deref())?;
    drop(session);
    let target = PathBuf::from(repo).join("out").join(name);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(target)?;
    file.write_all(b" changed")?;
    Ok(Response::Null)
}
