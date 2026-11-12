//! argparse-compatible parsing for the implemented paths. Every other path is an
//! invalid choice. Accepts `--opt=value`, a `--` terminator and unique prefixes.
use super::cli_feeds::{self, PAGING_OPTIONS, STATISTICS_OPTIONS};
pub use super::cli_feeds::{InteractionArgs, RecoveryArgs, StatisticsArgs};
use super::cli_history::{self, ARTIFACT_OPTIONS, EVENT_OPTIONS, PROMPT_OPTIONS};
pub use super::cli_history::{ArtifactArgs, EventArgs, PromptArgs};
use super::cli_pending::{self, PAGE_OPTIONS as PENDING_PAGE, STATUS_OPTIONS as PENDING_STATUS};
pub use super::cli_pending::{PendingPageArgs, PendingStatusArgs};
use super::cli_scan::{Opt, Scan, duration_seconds, error, int_option, py_repr, scan};

pub const PROGRAM: &str = "harflow-ctl";
pub const VERSION_PROGRAM: &str = "harflow-ctl version-info";
/// Commands that need no store.
pub const COMMANDS: [&str; 2] = ["version-info", "state-inspect"];
/// Commands that read the store, in the order the usage text lists them.
pub const READ_COMMANDS: [&str; 12] = [
    "list",
    "status",
    "status-batch",
    "task-list-page",
    "task-events",
    "task-prompt",
    "task-artifacts",
    "recovery-activity",
    "interaction-events",
    "statistics-page",
    "task-pending-page",
    "task-pending-status",
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageError {
    pub program: String,
    pub message: String,
}
/// The arguments of `task-list-page`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageArgs {
    pub json: bool,
    pub limit: i64,
    pub cursor: Option<String>,
    pub active_only: bool,
    pub session_id: Option<String>,
    pub task_ids: Option<String>,
    pub predecessor: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parsed {
    Help(Option<&'static str>),
    VersionInfo {
        json: bool,
    },
    StateInspect {
        json: bool,
    },
    List {
        json: bool,
        /// The `--since` window in seconds.
        since: Option<i64>,
    },
    Status {
        json: bool,
        task_id: String,
        observer_parent: bool,
        brief: bool,
    },
    StatusBatch {
        json: bool,
        task_ids: String,
        observer_parent: bool,
    },
    Page(PageArgs),
    Events(EventArgs),
    Prompt(PromptArgs),
    Artifacts(ArtifactArgs),
    Recovery(RecoveryArgs),
    Interactions(InteractionArgs),
    Statistics(StatisticsArgs),
    PendingPage(PendingPageArgs),
    PendingStatus(PendingStatusArgs),
}
fn unrecognized(program: &str, extras: &[String]) -> UsageError {
    error(
        program,
        format!("unrecognized arguments: {}", extras.join(" ")),
    )
}
/// `harflow-ctl version-info ...`: the direct entry, which has its own parser.
pub fn parse_version_info(args: &[String]) -> Result<Parsed, UsageError> {
    let parsed = scan(VERSION_PROGRAM, &[], args)?;
    if parsed.help {
        return Ok(Parsed::Help(Some("version-info")));
    }
    let mut extras = parsed.positionals;
    extras.extend(parsed.extras);
    if !extras.is_empty() {
        return Err(unrecognized(VERSION_PROGRAM, &extras));
    }
    Ok(Parsed::VersionInfo { json: parsed.json })
}
const OBSERVER: Opt = Opt {
    flag: "--observer-parent",
    value: false,
};
const PAGE_OPTIONS: [Opt; 6] = [
    Opt {
        flag: "--limit",
        value: true,
    },
    Opt {
        flag: "--cursor",
        value: true,
    },
    Opt {
        flag: "--active-only",
        value: false,
    },
    Opt {
        flag: "--session-id",
        value: true,
    },
    Opt {
        flag: "--task-ids",
        value: true,
    },
    Opt {
        flag: "--predecessor-task-id",
        value: true,
    },
];
const BRIEF: Opt = Opt {
    flag: "--brief",
    value: false,
};
const SINCE: Opt = Opt {
    flag: "--since",
    value: true,
};
const TASK_IDS: Opt = Opt {
    flag: "--task-ids",
    value: true,
};
fn required(program: &str, name: &str) -> UsageError {
    error(
        program,
        format!("the following arguments are required: {name}"),
    )
}
fn text(parsed: &Scan, flag: &str) -> Option<String> {
    parsed.value(flag).map(str::to_string)
}
/// Finishes a command: unrecognized tokens, including the surplus bare ones, are an error.
fn finish(
    mut extras: Vec<String>,
    parsed: &Scan,
    used: usize,
    parsed_value: Parsed,
) -> Result<Parsed, UsageError> {
    extras.extend(parsed.positionals.iter().skip(used).cloned());
    extras.extend(parsed.extras.iter().cloned());
    if extras.is_empty() {
        Ok(parsed_value)
    } else {
        Err(unrecognized(PROGRAM, &extras))
    }
}
fn parse_list(parsed: &Scan) -> Result<Parsed, UsageError> {
    let since = match parsed.value("--since") {
        Some(raw) => Some(duration_seconds(raw).ok_or_else(|| {
            error(
                "harflow-ctl list",
                format!(
                    "invalid duration {}; expected e.g. '7d', '12h', '30m'",
                    py_repr(raw)
                ),
            )
        })?),
        None => None,
    };
    Ok(Parsed::List {
        json: parsed.json,
        since,
    })
}
fn parse_page(program: &str, parsed: &Scan) -> Result<Parsed, UsageError> {
    let limit = match parsed.value("--limit") {
        Some(raw) => int_option(program, "--limit", raw)?,
        None => 100,
    };
    let args = PageArgs {
        json: parsed.json,
        limit,
        cursor: text(parsed, "--cursor"),
        active_only: parsed.flag("--active-only"),
        session_id: text(parsed, "--session-id"),
        task_ids: text(parsed, "--task-ids"),
        predecessor: text(parsed, "--predecessor-task-id"),
    };
    Ok(Parsed::Page(args))
}
/// Options before the command, then the command and the tokens after it.
type Split<'a> = (Vec<String>, &'a String, &'a [String]);
fn split_command(argv: &[String]) -> Result<Option<Split<'_>>, UsageError> {
    let mut extras = Vec::new();
    let mut index = 0;
    let mut command = None;
    while let Some(token) = argv.get(index) {
        index += 1;
        if token == "--" {
            command = argv.get(index);
            index += 1;
            break;
        }
        if !token.starts_with('-') || token == "-" {
            command = Some(token);
            break;
        }
        let long = token.strip_prefix("--");
        let name = long.map(|body| body.split_once('=').map_or(body, |pair| pair.0));
        let is_help = name.is_some_and(|name| !name.is_empty() && "help".starts_with(name));
        if token == "-h" || is_help {
            return Ok(None);
        }
        extras.push(token.clone());
    }
    let Some(command) = command else {
        let message = "the following arguments are required: command".to_string();
        return Err(error(PROGRAM, message));
    };
    Ok(Some((
        extras,
        command,
        argv.get(index..).unwrap_or_default(),
    )))
}
fn command_options(name: &str) -> Vec<Opt> {
    match name {
        "status" => vec![OBSERVER, BRIEF],
        "task-events" => EVENT_OPTIONS.to_vec(),
        "task-prompt" => PROMPT_OPTIONS.to_vec(),
        "task-artifacts" => ARTIFACT_OPTIONS.to_vec(),
        "recovery-activity" | "interaction-events" => PAGING_OPTIONS.to_vec(),
        "statistics-page" => STATISTICS_OPTIONS.to_vec(),
        "task-pending-page" => PENDING_PAGE.to_vec(),
        "task-pending-status" => PENDING_STATUS.to_vec(),
        "status-batch" => vec![TASK_IDS, OBSERVER],
        "list" => vec![SINCE],
        "task-list-page" => PAGE_OPTIONS.to_vec(),
        _ => Vec::new(),
    }
}
/// The value of a parsed command and how many bare tokens it consumed.
fn build(name: &str, program: &str, parsed: &Scan) -> Result<(Parsed, usize), UsageError> {
    let both = |value| Ok((value, 0));
    match name {
        "version-info" => both(Parsed::VersionInfo { json: parsed.json }),
        "state-inspect" => both(Parsed::StateInspect { json: parsed.json }),
        "list" => both(parse_list(parsed)?),
        "status" => {
            let task_id = parsed
                .positionals
                .first()
                .cloned()
                .ok_or_else(|| required(program, "task_id"))?;
            let value = Parsed::Status {
                json: parsed.json,
                task_id,
                observer_parent: parsed.flag("--observer-parent"),
                brief: parsed.flag("--brief"),
            };
            Ok((value, 1))
        }
        "status-batch" => {
            let task_ids =
                text(parsed, "--task-ids").ok_or_else(|| required(program, "--task-ids"))?;
            both(Parsed::StatusBatch {
                json: parsed.json,
                task_ids,
                observer_parent: parsed.flag("--observer-parent"),
            })
        }
        "task-events" => Ok((cli_history::events(program, parsed)?, 1)),
        "task-prompt" => Ok((cli_history::prompt(program, parsed)?, 1)),
        "task-artifacts" => Ok((cli_history::artifacts(program, parsed)?, 1)),
        "recovery-activity" => Ok((cli_feeds::recovery(program, parsed)?, 1)),
        "interaction-events" => both(cli_feeds::interactions(program, parsed)?),
        "statistics-page" => both(cli_feeds::statistics(program, parsed)?),
        "task-pending-page" => both(cli_pending::page(program, parsed)?),
        "task-pending-status" => both(cli_pending::status(program, parsed)?),
        _ => both(parse_page(program, parsed)?),
    }
}
pub fn parse_ctl(argv: &[String], reads: bool) -> Result<Parsed, UsageError> {
    let Some((extras, command, rest)) = split_command(argv)? else {
        return Ok(Parsed::Help(None));
    };
    let known = COMMANDS
        .iter()
        .chain(READ_COMMANDS.iter().filter(|_| reads))
        .find(|name| **name == command.as_str());
    let Some(name) = known.copied() else {
        let mut choices = COMMANDS.to_vec();
        if reads {
            choices.extend(READ_COMMANDS);
        }
        let choices = choices
            .iter()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let message =
            format!("argument command: invalid choice: '{command}' (choose from {choices})");
        return Err(error(PROGRAM, message));
    };
    let program = if name == "version-info" {
        VERSION_PROGRAM.to_string()
    } else {
        format!("{PROGRAM} {name}")
    };
    let parsed = scan(&program, &command_options(name), rest)?;
    if parsed.help {
        return Ok(Parsed::Help(Some(name)));
    }
    let (value, used) = build(name, &program, &parsed)?;
    let checked = finish(extras, &parsed, used, value)?;
    if let Parsed::Page(args) = &checked
        && args.predecessor.is_some()
        && args.task_ids.is_some()
    {
        let message = "--predecessor-task-id cannot be combined with --task-ids".to_string();
        return Err(error(PROGRAM, message));
    }
    cli_history::validate(&checked)?;
    Ok(checked)
}
pub fn help_text(command: Option<&str>) -> String {
    if let Some(text) =
        command.and_then(|name| cli_feeds::help(name).or_else(|| cli_pending::help(name)))
    {
        return text;
    }
    match command {
        Some("version-info") => "usage: harflow-ctl version-info [-h] [--json]\n".to_string(),
        Some("list") => "usage: harflow-ctl list [-h] [--since SINCE] [--json]\n".to_string(),
        Some("status") => concat!(
            "usage: harflow-ctl status [-h] [--json] [--observer-parent] [--brief]\n",
            "                          task_id\n"
        )
        .to_string(),
        Some("task-events") => concat!(
            "usage: harflow-ctl task-events [-h] [--from {newest,oldest} | --cursor CURSOR]\n",
            "                               [--limit LIMIT] [--kinds KINDS | --all-kinds]\n",
            "                               [--json]\n",
            "                               task_id\n"
        )
        .to_string(),
        Some("task-prompt") => concat!(
            "usage: harflow-ctl task-prompt [-h] [--max-bytes MAX_BYTES] [--json]\n",
            "                               task_id\n"
        )
        .to_string(),
        Some("task-artifacts") => concat!(
            "usage: harflow-ctl task-artifacts [-h] [--cursor CURSOR] [--limit LIMIT]\n",
            "                                  [--json]\n",
            "                                  task_id\n"
        )
        .to_string(),
        Some("status-batch") => concat!(
            "usage: harflow-ctl status-batch [-h] --task-ids TASK_IDS [--json]\n",
            "                                [--observer-parent]\n"
        )
        .to_string(),
        Some("task-list-page") => concat!(
            "usage: harflow-ctl task-list-page [-h] [--json] [--limit LIMIT] [--cursor CURSOR]\n",
            "                                  [--active-only] [--session-id SESSION_ID]\n",
            "                                  [--task-ids TASK_IDS]\n",
            "                                  [--predecessor-task-id PREDECESSOR_TASK_ID]\n"
        )
        .to_string(),
        Some(_) => "usage: harflow-ctl state-inspect [-h] [--json]\n".to_string(),
        None => concat!(
            "usage: harflow-ctl [-h] command ...\n\n",
            "commands:\n",
            "  version-info   report side-effect-free Harflow v1 compatibility\n",
            "  state-inspect  inspect state without mutation\n\n",
            "State root: HARFLOW_STATE_ROOT, otherwise the home directory's .harflow.\n",
        )
        .to_string(),
    }
}
