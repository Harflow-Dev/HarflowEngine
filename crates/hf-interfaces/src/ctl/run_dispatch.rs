use super::{
    dispatch::{CtlServices, Outcome},
    render,
};
use hf_application::ctl::task_run::{RunFailure, RunRequest};
use hf_domain::ctl::identity::CTL_FORMAT;
use serde_json::json;
const WARNING: &str = "harflow-ctl: experimental run/status checkpoint; cancellation is unavailable; launch acceptance is not completion.\n";
const CANCEL: &str = "Cancellation is unavailable in this experimental Rust run/status checkpoint.";
fn failure(message: &str, unknown: bool, json_mode: bool) -> Outcome {
    let doc = if unknown {
        json!({"unknown":{"message":message}})
    } else {
        json!({"error":{"code":"invalid_params","message":message}})
    };
    Outcome {
        stdout: if json_mode {
            render::line(&render::stamp(CTL_FORMAT, doc))
        } else {
            String::new()
        },
        stderr: if json_mode {
            String::new()
        } else {
            format!("harflow-ctl: error: {message}\n")
        },
        code: if unknown { 3 } else { 1 },
    }
}
pub(super) fn cancel(json: bool) -> Outcome {
    failure(CANCEL, false, json)
}
pub(super) fn run(services: &dyn CtlServices, request: &RunRequest, json_mode: bool) -> Outcome {
    if request.backend != "claude"
        || request.freedom != "unrestricted"
        || request.prompt.len() > 1_048_576
        || request.prompt.trim().is_empty()
        || request.prompt.contains('\0')
    {
        return failure(
            "This checkpoint requires Claude, explicit unrestricted freedom and a prompt of at most 1 MiB.",
            false,
            json_mode,
        );
    }
    let root = match services.resolve_state_root() {
        Ok(root) => root,
        Err(_) => return failure("Invalid state root.", false, json_mode),
    };
    match services.run_task(&root, request) {
        Ok(task_id) => Outcome {
            stdout: if json_mode {
                render::line(&render::stamp(
                    CTL_FORMAT,
                    json!({"result":{"task_id":task_id}}),
                ))
            } else {
                format!("{task_id}\n")
            },
            stderr: WARNING.into(),
            code: 0,
        },
        Err(RunFailure::Refused(message)) => failure(&message, false, json_mode),
        Err(RunFailure::Unknown(message)) => failure(&message, true, json_mode),
    }
}
