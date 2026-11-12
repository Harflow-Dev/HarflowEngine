//! Arguments of `task-pending-page` and `task-pending-status`. Usage errors here precede
//! every authority decision; nothing else is judged while parsing.
use super::{
    cli::{Parsed, UsageError},
    cli_scan::{Opt, Scan, error, int_option},
};
use hf_domain::ctl::pending::DEFAULT_LIMIT;

/// The arguments of `task-pending-page`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingPageArgs {
    pub json: bool,
    pub limit: i64,
    pub cursor: Option<String>,
}

/// The arguments of `task-pending-status`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingStatusArgs {
    pub json: bool,
    pub task_id: String,
}

pub(super) const PAGE_OPTIONS: [Opt; 2] = [
    Opt {
        flag: "--limit",
        value: true,
    },
    Opt {
        flag: "--cursor",
        value: true,
    },
];
pub(super) const STATUS_OPTIONS: [Opt; 1] = [Opt {
    flag: "--task-id",
    value: true,
}];

pub(super) fn page(program: &str, parsed: &Scan) -> Result<Parsed, UsageError> {
    let limit = match parsed.value("--limit") {
        Some(raw) => int_option(program, "--limit", raw)?,
        None => DEFAULT_LIMIT,
    };
    Ok(Parsed::PendingPage(PendingPageArgs {
        json: parsed.json,
        limit,
        cursor: parsed.value("--cursor").map(str::to_string),
    }))
}

pub(super) fn status(program: &str, parsed: &Scan) -> Result<Parsed, UsageError> {
    let task_id = parsed
        .value("--task-id")
        .map(str::to_string)
        .ok_or_else(|| {
            error(
                program,
                "the following arguments are required: --task-id".to_string(),
            )
        })?;
    Ok(Parsed::PendingStatus(PendingStatusArgs {
        json: parsed.json,
        task_id,
    }))
}

/// The usage text of these commands.
pub(super) fn help(command: &str) -> Option<String> {
    let text = match command {
        "task-pending-page" => concat!(
            "usage: harflow-ctl task-pending-page [-h] [--json] [--limit LIMIT]\n",
            "                                     [--cursor CURSOR]\n"
        ),
        "task-pending-status" => {
            "usage: harflow-ctl task-pending-status [-h] [--json] --task-id TASK_ID\n"
        }
        _ => return None,
    };
    Some(text.to_string())
}
