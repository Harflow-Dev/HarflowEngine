//! Exact, duplicate-refusing syntax for the initial execution subset.
use super::cli::{Parsed, UsageError};
use hf_application::ctl::task_run::RunRequest;
use std::collections::BTreeMap;
fn error(message: &str) -> UsageError {
    UsageError {
        program: "harflow-ctl".into(),
        message: message.into(),
    }
}
pub(super) fn parse(argv: &[String], enabled: bool) -> Option<Result<Parsed, UsageError>> {
    let command = argv.first()?.as_str();
    if (!enabled && command == "run") || !matches!(command, "run" | "cancel") {
        return None;
    }
    Some(parse_command(command, &argv[1..]))
}
fn parse_command(command: &str, args: &[String]) -> Result<Parsed, UsageError> {
    let mut values = BTreeMap::new();
    let mut json = false;
    let mut task = None;
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        index += 1;
        if matches!(arg.as_str(), "--help" | "-h") {
            return Ok(Parsed::Help(Some(if command == "run" {
                "run"
            } else {
                "cancel"
            })));
        }
        if arg == "--json" {
            if json {
                return Err(error("duplicate --json"));
            }
            json = true;
            continue;
        }
        if command == "cancel" && !arg.starts_with('-') && task.is_none() {
            task = Some(arg.clone());
            continue;
        }
        let (name, explicit) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(a, b)| (a, Some(b)));
        if command != "run" || !matches!(name, "--backend" | "--freedom" | "--repo" | "--prompt") {
            return Err(error("unrecognized or unsupported argument"));
        }
        if values.contains_key(name) {
            return Err(error("duplicate run option"));
        }
        let value = match explicit {
            Some(value) => value.to_owned(),
            None => {
                let value = args
                    .get(index)
                    .ok_or_else(|| error("run option requires a value"))?;
                if value.starts_with("--") {
                    return Err(error("run option requires a value"));
                }
                index += 1;
                value.clone()
            }
        };
        values.insert(name.to_owned(), value);
    }
    if command == "cancel" {
        return Ok(Parsed::Cancel {
            json,
            task_id: task.ok_or_else(|| error("task_id is required"))?,
        });
    }
    let mut required = |name| {
        values
            .remove(name)
            .ok_or_else(|| error("--backend, --freedom, --repo and --prompt are required"))
    };
    Ok(Parsed::Run {
        json,
        request: RunRequest {
            backend: required("--backend")?,
            freedom: required("--freedom")?,
            repo: required("--repo")?,
            prompt: required("--prompt")?,
        },
    })
}
