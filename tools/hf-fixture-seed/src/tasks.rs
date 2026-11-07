//! Task-level operations: add, live caller, damage and the seeded world.
use crate::{
    events,
    fail::{Fail, Result},
    identity, root,
    root::Context,
};
use hf_domain::{process::ProcessIdentity, task_read::TaskId};
use hf_interfaces::fixture_port::{
    IdentityState, LiveCaller, PortError, Projection, PromptRecord, Response, TaskSpec, World,
};
use hf_storage::{FixtureFacts, FixtureProcess, FixturePrompt, FixtureTask, Sha256Stream, Store};

pub fn seed_world(context: &Context, world: &World) -> Result<Response> {
    let store = root::initialize(context)?;
    let outcome = fill(context, &store, world);
    drop(store);
    root::finish(outcome, root::require_wal(&context.root))?;
    Ok(Response::Null)
}
fn fill(context: &Context, store: &Store, world: &World) -> Result<()> {
    for task in &world.tasks {
        add_task(context, store, task.clone())?;
    }
    for (id, rows) in &world.events {
        for row in rows {
            events::append_event(store, id, row)?;
        }
    }
    Ok(())
}
pub fn add_task(context: &Context, store: &Store, spec: TaskSpec) -> Result<Response> {
    let projection = spec.projection(None);
    write(context, store, spec, None, projection)
}
pub fn damage_task(store: &Store, id: &TaskId) -> Result<Response> {
    store.damage_task_fixture(id, deadline())?;
    Ok(Response::Null)
}
pub fn add_live_caller(context: &Context, store: &Store, caller: &LiveCaller) -> Result<Response> {
    let start = match caller.identity {
        IdentityState::Unsupported => return Ok(Response::Supported(false)),
        IdentityState::Unknown => None,
        IdentityState::Alive => Some(identity::read(caller.pid)?),
        IdentityState::Reused => Some(identity::different(&identity::read(caller.pid)?)?),
    };
    if start
        .as_ref()
        .is_some_and(|start: &ProcessIdentity| start.pid() != caller.pid)
    {
        return Err(Fail::new("the process identity belongs to a different pid"));
    }
    let process = FixtureProcess {
        pid: caller.pid,
        start,
    };
    let spec = caller.task_spec();
    let projection = spec.projection(Some(caller.process_identity_state()));
    write(context, store, spec, Some(process), projection)?;
    Ok(Response::Supported(true))
}
fn deadline() -> std::time::Instant {
    std::time::Instant::now() + std::time::Duration::from_secs(30)
}
fn write(
    context: &Context,
    store: &Store,
    mut spec: TaskSpec,
    process: Option<FixtureProcess>,
    projection: Projection,
) -> Result<Response> {
    let mut response = Response::Empty;
    if spec.real_workspace {
        let parent = context
            .root
            .parent()
            .ok_or_else(|| Fail::new("the root has no parent"))?;
        let repo = std::fs::canonicalize(parent)?
            .join("repos")
            .join(spec.id.as_str());
        std::fs::create_dir_all(&repo)?;
        spec.repo_path = repo
            .into_os_string()
            .into_string()
            .map_err(|_| Fail::new("the repository path is not valid UTF-8"))?;
        response = Response::RepoPath(spec.repo_path.clone());
    }
    let prompt = spec.prompt_record(&sha256_hex)?;
    let task = FixtureTask {
        facts: facts(&spec),
        header: spec.header_blob(),
        status: spec.status_blob(projection),
        prompt: prompt_row(prompt),
        process,
    };
    store.add_task_fixture(&task, deadline())?;
    Ok(response)
}
fn facts(spec: &TaskSpec) -> FixtureFacts {
    FixtureFacts {
        id: spec.id.clone(),
        backend: spec.backend.clone(),
        repo_path: spec.repo_path.clone(),
        session_id: spec.session_id.clone(),
        status: spec.status.clone(),
        exit_code: spec.exit_code,
        started_at: spec.started_at.clone(),
        started_at_us: spec.started_at_us,
        finished_at: spec.finished_at.clone(),
        finished_at_us: spec.finished_at_us,
        output_drained: spec.output_drained,
        title: spec.title.clone(),
        model: spec.model.clone(),
        continuation_of: spec.continuation_of.clone(),
        chain_root: spec.continuation_of.clone(),
    }
}
fn prompt_row(prompt: PromptRecord) -> FixturePrompt {
    FixturePrompt {
        inline_text: prompt.inline_text,
        preview: prompt.preview,
        claimed_sha256: prompt.claimed_sha256,
        source_name: prompt.source_name,
        blob: prompt.blob,
    }
}
fn sha256_hex(bytes: &[u8]) -> std::result::Result<String, PortError> {
    let failed = |_| PortError::new("cannot hash the prompt");
    let mut hash = Sha256Stream::with_limit(bytes.len()).map_err(failed)?;
    hash.update(bytes).map_err(failed)?;
    Ok(hash.finish().map_err(failed)?.as_str().to_owned())
}
