//! Runs the store-backed read commands and renders their outcomes.
use super::{
    cli::{PageArgs, Parsed},
    cursor,
    dispatch::{CtlServices, Outcome, invalid_choice},
    feed_dispatch, history_dispatch, read_docs,
    read_errors::{ErrorDoc, ReadPath, describe, inconsistent},
    render,
    task_docs::Inconsistent,
};
use hf_application::ctl::task_reads::{self as reads, CursorArg, PageMode, PageRequest, ReadError};
use hf_domain::ctl::{identity::CTL_FORMAT, state_root::StateRoot};
use serde_json::{Value, json};

pub(super) fn failure(json: bool, doc: &ErrorDoc) -> Outcome {
    let stdout = if json {
        render::line(&render::stamp(CTL_FORMAT, json!({"error": doc.object()})))
    } else {
        String::new()
    };
    Outcome {
        stdout,
        stderr: format!("harflow-ctl: error: {}\n", doc.message),
        code: 1,
    }
}

/// `body` is the envelope payload; `plain` is what a run without `--json` prints.
pub(super) fn success(json: bool, body: Value, plain: Value) -> Outcome {
    let stdout = if json {
        render::line(&render::stamp(CTL_FORMAT, body))
    } else {
        render::pretty(&plain)
    };
    Outcome {
        stdout,
        ..Outcome::default()
    }
}

pub(super) fn rendered<T>(
    json: bool,
    root: &StateRoot,
    outcome: Result<T, ReadError>,
    path: ReadPath,
    build: impl FnOnce(T) -> Result<(Value, Value), Inconsistent>,
) -> Outcome {
    match outcome {
        Err(error) => failure(json, &describe(&error, path, root.as_str())),
        Ok(value) => match build(value) {
            Ok((body, plain)) => success(json, body, plain),
            Err(Inconsistent) => failure(json, &inconsistent(root.as_str())),
        },
    }
}

fn page_request(args: &PageArgs) -> PageRequest {
    let mode = match (&args.task_ids, &args.predecessor) {
        (_, Some(raw)) => PageMode::Successors(raw.clone()),
        (Some(raw), None) => PageMode::TaskIds(raw.split(',').map(str::to_string).collect()),
        (None, None) => PageMode::Plain,
    };
    let cursor = match &args.cursor {
        None => CursorArg::Absent,
        Some(raw) => cursor::decode(raw).map_or(CursorArg::Malformed, CursorArg::Valid),
    };
    PageRequest {
        mode,
        limit: args.limit,
        cursor,
        active_only: args.active_only,
        session: args.session_id.clone(),
    }
}

/// `root` is the selected state root. The caller has already refused platforms without reads.
pub(super) fn run_read(parsed: &Parsed, root: &StateRoot, services: &dyn CtlServices) -> Outcome {
    let Some(read) = services.reads() else {
        return invalid_choice(command_name(parsed), false);
    };
    let now = services.now_micros();
    match parsed {
        Parsed::List { json, since } => {
            let window = since.map(|secs| secs.saturating_mul(1_000_000));
            let since_us = match (window, now) {
                (None, _) => None,
                (Some(window), Some(now)) => Some(now.saturating_sub(window)),
                (Some(_), None) => {
                    let message = "the clock is unavailable; --since cannot be applied";
                    let doc = ErrorDoc {
                        code: "invalid_params",
                        message: message.to_string(),
                        data: None,
                    };
                    return failure(*json, &doc);
                }
            };
            let outcome = reads::list(&read, root, since_us);
            rendered(*json, root, outcome, ReadPath::Managed, |views| {
                let body = read_docs::list_body(&views, now)?;
                Ok((body.clone(), body))
            })
        }
        Parsed::Status {
            json,
            task_id,
            observer_parent,
            brief,
        } => {
            let outcome = reads::status_with(&read, root, task_id, *brief);
            let parent = observer_parent.then(|| services.observer_parent());
            rendered(*json, root, outcome, ReadPath::Managed, |out| {
                let task = read_docs::status_task(&out, now, &parent, *brief)?;
                Ok((json!({"task": task.clone()}), task))
            })
        }
        Parsed::StatusBatch {
            json,
            task_ids,
            observer_parent,
        } => {
            let ids: Vec<String> = task_ids.split(',').map(str::to_string).collect();
            let outcome = reads::status_batch(&read, root, &ids);
            let parent = observer_parent.then(|| services.observer_parent());
            rendered(*json, root, outcome, ReadPath::Managed, |out| {
                let body = read_docs::batch_body(&out, now, &parent, root.as_str())?;
                Ok((body.clone(), body))
            })
        }
        Parsed::Page(args) => {
            let path = if args.predecessor.is_some() {
                ReadPath::Successors
            } else {
                ReadPath::Page
            };
            let outcome = reads::page_with(&read, root, &page_request(args), &|view| {
                read_docs::item_size(view, now)
            });
            rendered(args.json, root, outcome, path, |page| {
                let result = read_docs::page_body(&page, now)?;
                Ok((json!({"result": result.clone()}), result))
            })
        }
        Parsed::Events(args) => history_dispatch::events(&read, root, args, now),
        Parsed::Prompt(args) => history_dispatch::prompt(&read, root, args),
        Parsed::Artifacts(args) => history_dispatch::artifacts(&read, root, args, services, now),
        Parsed::Recovery(args) => feed_dispatch::recovery(&read, root, args),
        Parsed::Interactions(args) => feed_dispatch::interactions(&read, root, args),
        Parsed::Statistics(args) => feed_dispatch::statistics(&read, root, args, services),
        other => invalid_choice(command_name(other), false),
    }
}

fn command_name(parsed: &Parsed) -> &'static str {
    match parsed {
        Parsed::List { .. } => "list",
        Parsed::Status { .. } => "status",
        Parsed::StatusBatch { .. } => "status-batch",
        Parsed::Page(_) => "task-list-page",
        Parsed::Events(_) => "task-events",
        Parsed::Prompt(_) => "task-prompt",
        Parsed::Artifacts(_) => "task-artifacts",
        Parsed::Recovery(_) => "recovery-activity",
        Parsed::Interactions(_) => "interaction-events",
        Parsed::Statistics(_) => "statistics-page",
        Parsed::StateInspect { .. } => "state-inspect",
        Parsed::VersionInfo { .. } => "version-info",
        Parsed::Help(_) => "help",
    }
}
