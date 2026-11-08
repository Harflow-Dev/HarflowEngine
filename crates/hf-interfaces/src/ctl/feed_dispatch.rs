//! Runs `recovery-activity`, `interaction-events` and `statistics-page`.
use super::{
    cli::{InteractionArgs, RecoveryArgs, StatisticsArgs},
    dispatch::{CtlServices, Outcome},
    feed_cursor::{decode_feed, decode_statistics},
    feed_docs::{StatsDoc, feed_result, feed_size, recovery_body},
    py_float::{self, Exact},
    read_dispatch::{failure, success},
    read_errors::{ReadPath, describe},
    render,
};
use hf_application::ctl::task_reads::{
    self as reads, FeedCursorArg, FeedRequest, Reads, StatsCursorArg, StatsRequest,
};
use hf_domain::ctl::{
    identity::CTL_FORMAT,
    state_root::StateRoot,
    statistics_time::{bounds, retention},
};
use serde_json::{Value, json};

/// The compact envelope is the output with or without `--json`; errors print JSON only with it.
pub(super) fn recovery(read: &Reads<'_>, root: &StateRoot, args: &RecoveryArgs) -> Outcome {
    match reads::recovery_activity(read, root, &args.task_id) {
        Err(error) => failure(
            args.json,
            &describe(&error, ReadPath::Managed, root.as_str()),
        ),
        Ok(out) => success(true, recovery_body(&out), Value::Null),
    }
}

/// A result printed as Python writes it: the envelope with `--json`, the bare result indented
/// without.
fn emit(json: bool, body: Value, plain: Value, exact: [Option<Exact<'_>>; 2]) -> Outcome {
    let stdout = if json {
        py_float::line_with(&render::stamp(CTL_FORMAT, body), exact[0])
    } else {
        py_float::pretty_with(&plain, exact[1])
    };
    Outcome {
        stdout,
        ..Outcome::default()
    }
}

pub(super) fn interactions(read: &Reads<'_>, root: &StateRoot, args: &InteractionArgs) -> Outcome {
    let cursor = args
        .cursor
        .as_deref()
        .map_or(FeedCursorArg::Absent, decode_feed);
    let request = FeedRequest {
        limit: args.limit,
        cursor,
    };
    match reads::interaction_events(read, root, &request, &feed_size) {
        Err(error) => failure(
            args.json,
            &describe(&error, ReadPath::Interactions, root.as_str()),
        ),
        Ok(page) => {
            let result = feed_result(&page);
            emit(
                args.json,
                json!({"result": result.clone()}),
                result,
                [None, None],
            )
        }
    }
}

pub(super) fn statistics(
    read: &Reads<'_>,
    root: &StateRoot,
    args: &StatisticsArgs,
    services: &dyn CtlServices,
) -> Outcome {
    let window = bounds(&args.since, &args.before);
    let cursor = args
        .cursor
        .as_deref()
        .map_or(StatsCursorArg::Absent, |raw| {
            decode_statistics(raw, root.as_str(), window.as_ref())
        });
    let request = StatsRequest {
        bounds: window,
        limit: args.limit,
        cursor,
    };
    let doc = StatsDoc {
        root: root.as_str(),
        bounds: window,
        now: services.now_micros(),
        retention: retention(services.retention_days().as_deref()),
    };
    let (budget, rows) = (
        services.statistics_budget(),
        services.statistics_expire_after_rows(),
    );
    match reads::statistics_limited(read, root, &request, &doc, budget, rows) {
        Err(error) => failure(
            args.json,
            &describe(&error, ReadPath::Statistics, root.as_str()),
        ),
        Ok(out) => {
            let result = doc.result(&out, services.now_micros());
            let exact = [
                doc.exact("/result/retention/days"),
                doc.exact("/retention/days"),
            ];
            emit(
                args.json,
                json!({"v": 1, "result": result.clone()}),
                result,
                exact,
            )
        }
    }
}
