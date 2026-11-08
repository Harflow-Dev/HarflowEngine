//! Arguments of `recovery-activity`, `interaction-events` and `statistics-page`. Usage
//! errors here precede every authority decision; nothing else is judged while parsing.
use super::{
    cli::Parsed,
    cli::UsageError,
    cli_scan::{Opt, Scan, error, int_option},
};

/// The arguments of `recovery-activity`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryArgs {
    pub json: bool,
    pub task_id: String,
    pub cursor: Option<String>,
    pub limit: i64,
}

/// The arguments of `interaction-events`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionArgs {
    pub json: bool,
    pub cursor: Option<String>,
    pub limit: i64,
}

/// The arguments of `statistics-page`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatisticsArgs {
    pub json: bool,
    pub since: String,
    pub before: String,
    pub limit: i64,
    pub cursor: Option<String>,
}

const fn option(flag: &'static str) -> Opt {
    Opt { flag, value: true }
}

pub(super) const PAGING_OPTIONS: [Opt; 2] = [option("--cursor"), option("--limit")];
pub(super) const STATISTICS_OPTIONS: [Opt; 4] = [
    option("--since"),
    option("--before"),
    option("--limit"),
    option("--cursor"),
];

fn number(program: &str, parsed: &Scan, default: i64) -> Result<i64, UsageError> {
    parsed
        .value("--limit")
        .map_or(Ok(default), |raw| int_option(program, "--limit", raw))
}

fn cursor(parsed: &Scan) -> Option<String> {
    parsed.value("--cursor").map(str::to_string)
}

pub(super) fn recovery(program: &str, parsed: &Scan) -> Result<Parsed, UsageError> {
    let task_id = parsed.positionals.first().cloned().ok_or_else(|| {
        error(
            program,
            "the following arguments are required: task_id".to_string(),
        )
    })?;
    Ok(Parsed::Recovery(RecoveryArgs {
        json: parsed.json,
        task_id,
        cursor: cursor(parsed),
        limit: number(program, parsed, 50)?,
    }))
}

pub(super) fn interactions(program: &str, parsed: &Scan) -> Result<Parsed, UsageError> {
    Ok(Parsed::Interactions(InteractionArgs {
        json: parsed.json,
        cursor: cursor(parsed),
        limit: number(program, parsed, 50)?,
    }))
}

pub(super) fn statistics(program: &str, parsed: &Scan) -> Result<Parsed, UsageError> {
    let missing: Vec<&str> = ["--since", "--before"]
        .into_iter()
        .filter(|flag| parsed.value(flag).is_none())
        .collect();
    if !missing.is_empty() {
        let message = format!(
            "the following arguments are required: {}",
            missing.join(", ")
        );
        return Err(error(program, message));
    }
    let bound = |flag: &str| parsed.value(flag).unwrap_or_default().to_string();
    Ok(Parsed::Statistics(StatisticsArgs {
        json: parsed.json,
        since: bound("--since"),
        before: bound("--before"),
        limit: number(program, parsed, 100)?,
        cursor: cursor(parsed),
    }))
}

/// The usage text of these commands.
pub(super) fn help(command: &str) -> Option<String> {
    let text = match command {
        "recovery-activity" => concat!(
            "usage: harflow-ctl recovery-activity [-h] [--json] [--cursor CURSOR]\n",
            "                                     [--limit LIMIT]\n",
            "                                     task_id\n"
        ),
        "interaction-events" => concat!(
            "usage: harflow-ctl interaction-events [-h] [--cursor CURSOR] [--limit LIMIT]\n",
            "                                      [--json]\n"
        ),
        "statistics-page" => concat!(
            "usage: harflow-ctl statistics-page [-h] --since SINCE --before BEFORE\n",
            "                                   [--limit LIMIT] [--cursor CURSOR] [--json]\n"
        ),
        _ => return None,
    };
    Some(text.to_string())
}
