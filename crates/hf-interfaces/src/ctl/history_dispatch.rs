//! Runs the history reads: `task-events`, `task-prompt` and `task-artifacts`.
use super::{
    cli::{ArtifactArgs, EventArgs, PromptArgs},
    cli_history::kind_list,
    dispatch::{CtlServices, Outcome, invalid_choice},
    event_cursor, event_docs, history_docs,
    read_dispatch::rendered,
    read_errors::ReadPath,
};
use hf_application::ctl::task_reads::{
    self as reads, ArtifactRequest, EventCursorArg, EventsRequest, Reads,
};
use hf_domain::{ctl::state_root::StateRoot, event_read::EventFilter};
use serde_json::json;

/// Events and prompts are printed as JSON only; usage errors have already refused the rest.
pub(super) fn events(
    read: &Reads<'_>,
    root: &StateRoot,
    args: &EventArgs,
    now: Option<i64>,
) -> Outcome {
    let filter = match (&args.kinds, args.all_kinds) {
        (Some(raw), _) => {
            let mut kinds = kind_list(raw).unwrap_or_default();
            kinds.sort();
            kinds.dedup();
            EventFilter::Kinds(kinds)
        }
        (None, true) => EventFilter::All,
        (None, false) => EventFilter::Default,
    };
    let request = EventsRequest {
        task_id: args.task_id.clone(),
        limit: usize::try_from(args.limit).unwrap_or(0),
        filter: filter.clone(),
        from_oldest: args.from_oldest,
        cursor: args
            .cursor
            .as_deref()
            .map_or(EventCursorArg::Absent, event_cursor::decode),
    };
    let outcome = reads::events(read, root, &request, &|out| {
        event_docs::fits(out, &filter, now)
    });
    rendered(true, root, outcome, ReadPath::History, |out| {
        let result = event_docs::event_result(&out, &filter, now)?;
        Ok((json!({"result": result.clone()}), result))
    })
}

pub(super) fn prompt(read: &Reads<'_>, root: &StateRoot, args: &PromptArgs) -> Outcome {
    let max_bytes = usize::try_from(args.max_bytes).unwrap_or(0);
    let outcome = reads::prompt(read, root, &args.task_id, max_bytes);
    rendered(true, root, outcome, ReadPath::History, |out| {
        let result = history_docs::prompt_result(&out);
        Ok((json!({"result": result.clone()}), result))
    })
}

pub(super) fn artifacts(
    read: &Reads<'_>,
    root: &StateRoot,
    args: &ArtifactArgs,
    services: &dyn CtlServices,
    now: Option<i64>,
) -> Outcome {
    let Some(targets) = services.artifact_targets() else {
        return invalid_choice("task-artifacts", false);
    };
    let request = ArtifactRequest {
        task_id: args.task_id.clone(),
        limit: args.limit,
        cursor: args.cursor.as_deref().map_or(
            reads::ArtifactCursorArg::Absent,
            history_docs::decode_cursor,
        ),
        root_task_marker: services.root_task_marker(),
    };
    let outcome = reads::artifacts(read, targets, root, &request, &|task, entries| {
        history_docs::artifact_size(task, true, entries, now)
    });
    rendered(args.json, root, outcome, ReadPath::Artifacts, |page| {
        let result = history_docs::artifact_result(&page, now);
        Ok((json!({"result": result.clone()}), result))
    })
}
