//! Task-level operations: add, live caller, damage and the seeded world.
use crate::{
    events,
    fail::{Fail, Result},
    identity, root,
    root::Context,
};
use hf_domain::{process::ProcessIdentity, task_read::TaskId};
use hf_interfaces::fixture_port::{
    ChainRootSpec, IdentityState, LiveCaller, PortError, Projection, PromptRecord, Response,
    TaskSpec, World,
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
    let known: Vec<TaskId> = world.tasks.iter().map(|task| task.id.clone()).collect();
    for task in &world.tasks {
        add_task(context, store, task.clone(), &known)?;
    }
    for (id, rows) in &world.events {
        for row in rows {
            events::append_event(store, id, row)?;
        }
    }
    Ok(())
}
pub fn add_task(
    context: &Context,
    store: &Store,
    spec: TaskSpec,
    world: &[TaskId],
) -> Result<Response> {
    let projection = spec.projection(None);
    write(context, store, spec, None, projection, world)
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
    write(context, store, spec, Some(process), projection, &[])?;
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
    world: &[TaskId],
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
    let chain_root = chain_root(store, &spec, world)?;
    let task = FixtureTask {
        facts: facts(&spec, chain_root.clone()),
        header: spec.header_blob(chain_root.as_deref()),
        status: spec.status_blob(projection),
        prompt: prompt_row(prompt),
        process,
    };
    store.add_task_fixture(&task, deadline())?;
    Ok(response)
}
/// The stored chain root. An explicit root (none or text) is stored as given and needs no
/// predecessor. Otherwise it is the predecessor's stored root, else the predecessor itself
/// (the reference store); a predecessor that stores no usable root makes the predecessor the
/// root. A missing or damaged predecessor fails the operation, as the reference adapter
/// does, unless it is named by another task of the world being seeded: then it is the root.
/// The explicit form is a superset of the reference adapter, which refuses it for a missing
/// predecessor; it lets tests model broken ancestry.
fn chain_root(store: &Store, spec: &TaskSpec, world: &[TaskId]) -> Result<Option<String>> {
    match &spec.chain_root {
        ChainRootSpec::Absent => Ok(None),
        ChainRootSpec::Verbatim(text) => Ok(Some(text.clone())),
        ChainRootSpec::Default => {
            let Some(predecessor) = &spec.continuation_of else {
                return Ok(None);
            };
            let stored = match store.fixture_chain_root(predecessor, deadline())? {
                Some(stored) => stored,
                None if world.contains(predecessor) => None,
                None => return Err(Fail::new("the predecessor task is missing or damaged")),
            };
            Ok(Some(
                stored
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| predecessor.as_str().to_owned()),
            ))
        }
    }
}
fn facts(spec: &TaskSpec, chain_root: Option<String>) -> FixtureFacts {
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
        chain_root,
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
