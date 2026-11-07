//! Arguments of the history reads: `task-events`, `task-prompt` and `task-artifacts`.
//! Usage errors here precede every authority decision.
use super::{
    cli::{PROGRAM, Parsed, UsageError},
    cli_scan::{Opt, Scan, error, int_option, py_repr},
};
use hf_domain::event_read::EVENT_KINDS;

const EVENT_LIMIT_MAX: i64 = 200;
const MAX_BYTES_MAX: i64 = 131_072;

/// The arguments of `task-events`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventArgs {
    pub json: bool,
    pub task_id: String,
    pub from_oldest: bool,
    pub cursor: Option<String>,
    pub limit: i64,
    /// The raw comma-separated kinds.
    pub kinds: Option<String>,
    pub all_kinds: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptArgs {
    pub json: bool,
    pub task_id: String,
    pub max_bytes: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactArgs {
    pub json: bool,
    pub task_id: String,
    pub cursor: Option<String>,
    pub limit: i64,
}

const fn option(flag: &'static str, value: bool) -> Opt {
    Opt { flag, value }
}

pub(super) const EVENT_OPTIONS: [Opt; 5] = [
    option("--from", true),
    option("--cursor", true),
    option("--limit", true),
    option("--kinds", true),
    option("--all-kinds", false),
];
pub(super) const PROMPT_OPTIONS: [Opt; 1] = [option("--max-bytes", true)];
pub(super) const ARTIFACT_OPTIONS: [Opt; 2] = [option("--cursor", true), option("--limit", true)];

fn exclusive(program: &str, first: &str, second: &str) -> UsageError {
    error(
        program,
        format!("argument {second}: not allowed with argument {first}"),
    )
}

fn task_id(program: &str, parsed: &Scan) -> Result<String, UsageError> {
    parsed.positionals.first().cloned().ok_or_else(|| {
        error(
            program,
            "the following arguments are required: task_id".to_string(),
        )
    })
}

fn number(program: &str, parsed: &Scan, flag: &str, default: i64) -> Result<i64, UsageError> {
    parsed
        .value(flag)
        .map_or(Ok(default), |raw| int_option(program, flag, raw))
}

pub(super) fn events(program: &str, parsed: &Scan) -> Result<Parsed, UsageError> {
    let from = parsed.value("--from");
    if let Some(origin) = from
        && !["newest", "oldest"].contains(&origin)
    {
        let message = format!(
            "argument --from: invalid choice: {} (choose from 'newest', 'oldest')",
            py_repr(origin)
        );
        return Err(error(program, message));
    }
    if from.is_some() && parsed.value("--cursor").is_some() {
        return Err(exclusive(program, "--from", "--cursor"));
    }
    if parsed.value("--kinds").is_some() && parsed.flag("--all-kinds") {
        return Err(exclusive(program, "--kinds", "--all-kinds"));
    }
    Ok(Parsed::Events(EventArgs {
        json: parsed.json,
        task_id: task_id(program, parsed)?,
        from_oldest: from == Some("oldest"),
        cursor: parsed.value("--cursor").map(str::to_string),
        limit: number(program, parsed, "--limit", 100)?,
        kinds: parsed.value("--kinds").map(str::to_string),
        all_kinds: parsed.flag("--all-kinds"),
    }))
}

pub(super) fn prompt(program: &str, parsed: &Scan) -> Result<Parsed, UsageError> {
    Ok(Parsed::Prompt(PromptArgs {
        json: parsed.json,
        task_id: task_id(program, parsed)?,
        max_bytes: number(program, parsed, "--max-bytes", 65_536)?,
    }))
}

pub(super) fn artifacts(program: &str, parsed: &Scan) -> Result<Parsed, UsageError> {
    Ok(Parsed::Artifacts(ArtifactArgs {
        json: parsed.json,
        task_id: task_id(program, parsed)?,
        cursor: parsed.value("--cursor").map(str::to_string),
        limit: number(program, parsed, "--limit", 20)?,
    }))
}

fn refuse(message: String) -> UsageError {
    error(PROGRAM, message)
}

/// The kinds named by `--kinds`, each of which must be a known event kind.
pub fn kind_list(raw: &str) -> Result<Vec<String>, UsageError> {
    let kinds: Vec<String> = raw.split(',').map(str::to_string).collect();
    let mut unknown: Vec<&String> = kinds
        .iter()
        .filter(|kind| !EVENT_KINDS.contains(&kind.as_str()))
        .collect();
    unknown.sort();
    unknown.dedup();
    if unknown.is_empty() {
        return Ok(kinds);
    }
    let names: Vec<String> = unknown.iter().map(|kind| py_repr(kind)).collect();
    Err(refuse(format!("unknown event kinds: {}", names.join(", "))))
}

/// The checks that follow parsing: `--json` is required, then the ranges.
pub(super) fn validate(parsed: &Parsed) -> Result<(), UsageError> {
    match parsed {
        Parsed::Events(args) => {
            require_json(args.json, "task-events")?;
            if !(1..=EVENT_LIMIT_MAX).contains(&args.limit) {
                return Err(refuse(format!("--limit must be 1..{EVENT_LIMIT_MAX}")));
            }
            args.kinds.as_deref().map(kind_list).transpose().map(|_| ())
        }
        Parsed::Prompt(args) => {
            require_json(args.json, "task-prompt")?;
            if (1..=MAX_BYTES_MAX).contains(&args.max_bytes) {
                Ok(())
            } else {
                Err(refuse(format!("--max-bytes must be 1..{MAX_BYTES_MAX}")))
            }
        }
        _ => Ok(()),
    }
}

fn require_json(json: bool, command: &str) -> Result<(), UsageError> {
    if json {
        Ok(())
    } else {
        Err(refuse(format!("{command} requires --json")))
    }
}
