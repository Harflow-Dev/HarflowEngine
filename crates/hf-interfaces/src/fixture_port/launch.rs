//! Chain-stop and launch-receipt requests of the fixture port. Values stay typed here; the
//! seed tool turns them into store rows.
use super::{PortError, args::Args, spec::task_id};
use hf_domain::task_read::TaskId;
use serde_json::Value;

/// Bytes appended to an attribution to make it exceed the attribution read limit.
const OVERSIZE_PADDING: usize = 4096;
pub const LAUNCH_STATES: [&str; 7] = [
    "reserved",
    "claimed",
    "waiting_for_resources",
    "granted",
    "spawned",
    "settled",
    "not_started",
];
pub const REFUSAL_CHECKS: [&str; 8] = [
    "format",
    "version",
    "root_mismatch",
    "task_id_mismatch",
    "generation",
    "oversize",
    "field_shape",
    "resource_names",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopMode {
    /// A stable regular marker. `cancelled` is the raw attribution text as stored: `None`
    /// when no attribution could be read (an unparseable marker), a value that is not a
    /// task identity when invalid, and beyond the read limit when oversize.
    Marker { cancelled: Option<String> },
    /// The marker name exists but is not a regular file.
    Fault,
    /// No marker.
    Absent,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopSpec {
    pub chain_root: TaskId,
    pub mode: StopMode,
}
impl StopSpec {
    pub(super) fn parse(value: Value) -> Result<Self, PortError> {
        let keys = ["chain_root", "mode", "attribution"];
        let args = Args::new(value, "stop spec", &keys, &keys[..2])?;
        let mode = match args.string("mode")?.as_str() {
            "absent" => StopMode::Absent,
            "fault" => StopMode::Fault,
            "marker" => StopMode::Marker {
                cancelled: attribution(&args)?,
            },
            other => return Err(PortError::new(format!("unknown stop mode {other:?}"))),
        };
        Ok(Self {
            chain_root: task_id(args.string("chain_root")?)?,
            mode,
        })
    }
}
fn attribution(args: &Args) -> Result<Option<String>, PortError> {
    let keys = ["kind", "cancelled_task_id"];
    let inner = Args::new(
        args.object_value("attribution")?,
        "attribution",
        &keys,
        &keys,
    )?;
    let cancelled = inner.opt_string("cancelled_task_id")?;
    let required = || {
        cancelled
            .clone()
            .ok_or_else(|| PortError::new("attribution needs a cancelled_task_id"))
    };
    match inner.string("kind")?.as_str() {
        "valid" => {
            task_id(required()?)?;
            Ok(cancelled)
        }
        "invalid" => required().map(Some),
        "malformed" => Ok(None),
        "oversize" => Ok(Some(format!(
            "{}{}",
            required()?,
            "x".repeat(OVERSIZE_PADDING)
        ))),
        other => Err(PortError::new(format!(
            "unknown attribution kind {other:?}"
        ))),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lineage {
    pub spawned_by: Option<String>,
    pub root_task_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub chain_root_task_id: Option<String>,
}
/// A receipt's `resources` object. `names` is `None` when the object has no list of strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resources {
    pub names: Option<Vec<String>>,
}
/// A receipt's `worktree` object. `path` is `None` when the object has no string path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchSpec {
    pub name: TaskId,
    pub state: String,
    pub generation: String,
    pub backend: String,
    pub title: Option<String>,
    pub repo_path: Option<String>,
    pub created_at: Option<String>,
    pub not_started_reason: Option<String>,
    pub lineage: Lineage,
    pub resources: Option<Resources>,
    pub worktree: Option<Worktree>,
    /// One of `REFUSAL_CHECKS`: the receipt then fails exactly that qualification.
    pub refusal: Option<String>,
}
const LAUNCH_KEYS: [&str; 12] = [
    "name",
    "state",
    "generation",
    "backend",
    "title",
    "repo_path",
    "created_at",
    "not_started_reason",
    "lineage",
    "resources",
    "worktree",
    "refusal",
];
impl LaunchSpec {
    pub(super) fn parse(value: Value) -> Result<Self, PortError> {
        let args = Args::new(value, "launch spec", &LAUNCH_KEYS, &LAUNCH_KEYS)?;
        let state = args.string("state")?;
        if !LAUNCH_STATES.contains(&state.as_str()) {
            return Err(PortError::new(format!("unknown launch state {state:?}")));
        }
        Ok(Self {
            name: task_id(args.string("name")?)?,
            state,
            generation: args.string("generation")?,
            backend: args.string("backend")?,
            title: args.opt_string("title")?,
            repo_path: args.opt_string("repo_path")?,
            created_at: args.opt_string("created_at")?,
            not_started_reason: args.opt_string("not_started_reason")?,
            lineage: lineage(&args)?,
            resources: optional_object(&args, "resources")?.map(|object| Resources {
                names: string_list(object.get("names")),
            }),
            worktree: optional_object(&args, "worktree")?.map(|object| Worktree {
                path: object
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            }),
            refusal: refusal(&args)?,
        })
    }
}
fn optional_object(
    args: &Args,
    key: &str,
) -> Result<Option<serde_json::Map<String, Value>>, PortError> {
    if args.is_null(key) {
        return Ok(None);
    }
    args.object(key).map(|object| Some(object.clone()))
}
fn string_list(value: Option<&Value>) -> Option<Vec<String>> {
    value?
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}
fn lineage(args: &Args) -> Result<Lineage, PortError> {
    if args.is_null("lineage") {
        return Ok(Lineage::default());
    }
    let keys = [
        "spawned_by",
        "root_task_id",
        "parent_task_id",
        "chain_root_task_id",
    ];
    let inner = Args::new(args.object_value("lineage")?, "lineage", &keys, &[])?;
    Ok(Lineage {
        spawned_by: inner.opt_string("spawned_by")?,
        root_task_id: inner.opt_string("root_task_id")?,
        parent_task_id: inner.opt_string("parent_task_id")?,
        chain_root_task_id: inner.opt_string("chain_root_task_id")?,
    })
}
fn refusal(args: &Args) -> Result<Option<String>, PortError> {
    if args.is_null("refusal") {
        return Ok(None);
    }
    let inner = Args::new(
        args.object_value("refusal")?,
        "refusal",
        &["check"],
        &["check"],
    )?;
    let check = inner.string("check")?;
    if !REFUSAL_CHECKS.contains(&check.as_str()) {
        return Err(PortError::new(format!("unknown refusal check {check:?}")));
    }
    Ok(Some(check))
}
/// The name of a receipt file whose stem is not a task identity.
pub(super) fn invalid_name(name: String) -> Result<String, PortError> {
    if TaskId::new(name.clone()).is_ok() || name.is_empty() || name.contains(['/', '\0']) {
        return Err(PortError::new(
            "an invalid launch name must be a non-empty single component that is not a task identity",
        ));
    }
    Ok(name)
}
