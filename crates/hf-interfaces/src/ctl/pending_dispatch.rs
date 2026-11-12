//! Runs `task-pending-page` and `task-pending-status`.
use super::{
    bound::python_size,
    cli::{PendingPageArgs, PendingStatusArgs},
    dispatch::{CtlServices, Outcome},
    pending_cursor, pending_docs,
    read_dispatch::{failure, success},
    read_errors::{ReadPath, describe},
    read_errors_pending,
};
use hf_application::ctl::task_reads::{
    self as reads, PendingCursorArg, PendingFailure, PendingPageRequest, Reads,
};
use hf_domain::ctl::{
    pending::{ITEMS_RESERVE, MAX_RESPONSE_BYTES, RESULT_RESERVE},
    state_root::StateRoot,
};
use serde_json::{Value, json};

fn too_large(json_requested: bool) -> Outcome {
    let bound = PendingFailure::Check {
        code: "pending_observation_bound",
        check: "response_bytes",
    };
    failure(json_requested, &read_errors_pending::doc(&bound))
}

/// A result held to the response bound, then printed.
fn answer(json_requested: bool, result: Value, limit: usize) -> Outcome {
    if python_size(&result) > limit {
        return too_large(json_requested);
    }
    success(json_requested, json!({"result": result.clone()}), result)
}

pub(super) fn page(
    read: &Reads<'_>,
    root: &StateRoot,
    args: &PendingPageArgs,
    services: &dyn CtlServices,
) -> Outcome {
    let request = PendingPageRequest {
        limit: args.limit,
        cursor: args
            .cursor
            .as_deref()
            .map_or(PendingCursorArg::Absent, pending_cursor::decode),
    };
    let limits = services.pending_limits();
    match reads::pending_page(read, root, &request, &limits, &pending_docs::item_size) {
        Err(error) => failure(
            args.json,
            &describe(&error, ReadPath::Managed, root.as_str()),
        ),
        Ok(out) => {
            let result = pending_docs::page_result(&out, services.now_micros());
            answer(args.json, result, MAX_RESPONSE_BYTES - RESULT_RESERVE)
        }
    }
}

pub(super) fn status(
    read: &Reads<'_>,
    root: &StateRoot,
    args: &PendingStatusArgs,
    services: &dyn CtlServices,
) -> Outcome {
    let limits = services.pending_limits();
    match reads::pending_status(read, root, &args.task_id, &limits) {
        Err(error) => failure(
            args.json,
            &describe(&error, ReadPath::Managed, root.as_str()),
        ),
        Ok(out) => {
            let result = pending_docs::status_result(&out, services.now_micros());
            answer(args.json, result, MAX_RESPONSE_BYTES - ITEMS_RESERVE)
        }
    }
}
