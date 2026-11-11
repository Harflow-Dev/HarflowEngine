//! Typed requests and responses of the E1 conformance fixture port.
//!
//! Operation names, argument names and result shapes follow the port interface; this module
//! owns all JSON so that the seed tool itself handles typed values only. It reads no files and
//! has no clock.
mod args;
mod launch;
mod parts;
mod spec;
use args::Args;
use hf_domain::task_read::TaskId;
pub use launch::{
    LAUNCH_STATES, LaunchSpec, Lineage, REFUSAL_CHECKS, Resources, StopMode, StopSpec, Worktree,
};
pub use parts::{
    ArtifactSpec, CallerStatus, EventSpec, IdentityState, LiveCaller, Observation, Target,
    check_name,
};
use serde_json::Value;
pub use spec::{
    ChainRootSpec, PREVIEW_CHARS, Projection, PromptRecord, PromptStore, TaskSpec, header_repo_path,
};
/// Largest request body the port accepts.
pub const MAX_REQUEST: usize = 4 * 1024 * 1024;
pub const OPERATIONS: [&str; 20] = [
    "initialize_empty",
    "remove_root",
    "make_foreign_root",
    "root_state",
    "seed_world",
    "add_task",
    "add_live_caller",
    "damage_task",
    "append_event",
    "append_malformed_line",
    "append_partial_line",
    "truncate_log",
    "remove_log",
    "tamper_prompt",
    "write_artifacts",
    "change_artifact_target",
    "authoritative_digest",
    "stop_chain",
    "add_launch",
    "add_invalid_launch_name",
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortError(String);
impl PortError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
    pub fn message(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for PortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for PortError {}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct World {
    pub tasks: Vec<TaskSpec>,
    pub events: Vec<(TaskId, Vec<EventSpec>)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    InitializeEmpty,
    RemoveRoot,
    MakeForeignRoot,
    RootState,
    SeedWorld(World),
    AddTask(Box<TaskSpec>),
    AddLiveCaller(LiveCaller),
    DamageTask(TaskId),
    AppendEvent(TaskId, EventSpec),
    AppendMalformedLine(TaskId),
    AppendPartialLine(TaskId),
    TruncateLog(TaskId, u64),
    RemoveLog(TaskId),
    TamperPrompt(TaskId),
    WriteArtifacts {
        task: TaskId,
        items: Vec<ArtifactSpec>,
        header_valid: bool,
    },
    ChangeArtifactTarget(TaskId, String),
    AuthoritativeDigest,
    StopChain(StopSpec),
    AddLaunch(Box<LaunchSpec>),
    AddInvalidLaunchName(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Response {
    Null,
    /// A bare JSON string: a root state or a digest.
    Text(String),
    Empty,
    RepoPath(String),
    Supported(bool),
    Paths(Vec<String>),
}
/// Renders the single JSON value written to standard output.
pub fn render(response: &Response) -> String {
    let value = match response {
        Response::Null => Value::Null,
        Response::Text(text) => Value::String(text.clone()),
        Response::Empty => serde_json::json!({}),
        Response::RepoPath(path) => serde_json::json!({ "repo_path": path }),
        Response::Supported(flag) => serde_json::json!({ "supported": flag }),
        Response::Paths(paths) => serde_json::json!({ "paths": paths }),
    };
    value.to_string()
}
/// Decodes one operation's stdin. Missing keys are refused, never defaulted; the one
/// exception is `header_valid`, which the cases omit.
pub fn parse(operation: &str, stdin: &[u8]) -> Result<Request, PortError> {
    if stdin.len() > MAX_REQUEST {
        return Err(PortError::new("request too large"));
    }
    let value = if stdin.iter().all(u8::is_ascii_whitespace) {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_slice(stdin).map_err(|_| PortError::new("request is not JSON"))?
    };
    let task =
        |args: &Args| -> Result<TaskId, PortError> { spec::task_id(args.string("task_id")?) };
    let with = |keys: &[&str]| Args::new(value.clone(), "request", keys, keys);
    Ok(match operation {
        "initialize_empty" => none(value, Request::InitializeEmpty)?,
        "remove_root" => none(value, Request::RemoveRoot)?,
        "make_foreign_root" => none(value, Request::MakeForeignRoot)?,
        "root_state" => none(value, Request::RootState)?,
        "authoritative_digest" => none(value, Request::AuthoritativeDigest)?,
        "seed_world" => Request::SeedWorld(world(&with(&["world"])?)?),
        "add_task" => {
            let args = with(&["spec"])?;
            Request::AddTask(Box::new(TaskSpec::parse(args.object_value("spec")?)?))
        }
        "add_live_caller" => Request::AddLiveCaller(LiveCaller::parse(&with(&[
            "task_id", "pid", "identity", "status", "marker",
        ])?)?),
        "damage_task" => Request::DamageTask(task(&with(&["task_id"])?)?),
        "append_event" => {
            let args = with(&["task_id", "seq", "kind", "observed_at", "fields"])?;
            Request::AppendEvent(task(&args)?, EventSpec::from_args(&args)?)
        }
        "append_malformed_line" => Request::AppendMalformedLine(task(&with(&["task_id"])?)?),
        "append_partial_line" => Request::AppendPartialLine(task(&with(&["task_id"])?)?),
        "truncate_log" => {
            let args = with(&["task_id", "keep_events"])?;
            Request::TruncateLog(task(&args)?, args.u64("keep_events")?)
        }
        "remove_log" => Request::RemoveLog(task(&with(&["task_id"])?)?),
        "tamper_prompt" => Request::TamperPrompt(task(&with(&["task_id"])?)?),
        "write_artifacts" => artifacts(&value)?,
        "change_artifact_target" => {
            let args = with(&["task_id", "name"])?;
            let name = args.string("name")?;
            check_name(&name)?;
            Request::ChangeArtifactTarget(task(&args)?, name)
        }
        "stop_chain" => {
            Request::StopChain(StopSpec::parse(with(&["spec"])?.object_value("spec")?)?)
        }
        "add_launch" => Request::AddLaunch(Box::new(LaunchSpec::parse(
            with(&["spec"])?.object_value("spec")?,
        )?)),
        "add_invalid_launch_name" => {
            Request::AddInvalidLaunchName(launch::invalid_name(with(&["name"])?.string("name")?)?)
        }
        other => return Err(PortError::new(format!("unknown operation {other:?}"))),
    })
}
fn none(value: Value, request: Request) -> Result<Request, PortError> {
    Args::new(value, "request", &[], &[])?;
    Ok(request)
}
fn artifacts(value: &Value) -> Result<Request, PortError> {
    let keys = ["task_id", "items", "header_valid"];
    let args = Args::new(value.clone(), "request", &keys, &keys[..2])?;
    let items = args
        .array("items")?
        .iter()
        .map(|item| ArtifactSpec::parse(item.clone()))
        .collect::<Result<_, _>>()?;
    Ok(Request::WriteArtifacts {
        task: spec::task_id(args.string("task_id")?)?,
        items,
        header_valid: args.opt_bool("header_valid")?.unwrap_or(true),
    })
}
fn world(args: &Args) -> Result<World, PortError> {
    let world = Args::new(
        args.object_value("world")?,
        "world",
        &["tasks", "events"],
        &["tasks", "events"],
    )?;
    let tasks = world
        .array("tasks")?
        .iter()
        .map(|task| TaskSpec::parse(task.clone()))
        .collect::<Result<_, _>>()?;
    let mut events = Vec::new();
    for (id, rows) in world.object("events")? {
        let rows = rows
            .as_array()
            .ok_or_else(|| PortError::new("world events must be arrays"))?;
        let rows = rows
            .iter()
            .map(|row| EventSpec::parse(row.clone()))
            .collect::<Result<_, _>>()?;
        events.push((spec::task_id(id.clone())?, rows));
    }
    Ok(World { tasks, events })
}
