//! Chain-stop markers and launch receipts: the typed spec becomes a store row, qualifying
//! it the way the reference engine qualifies the receipt document and then the pending page.
use crate::{fail::Result, root::Context};
use hf_domain::ctl::pending::names_valid;
use hf_interfaces::fixture_port::{LaunchSpec, Response, StopMode, StopSpec};
use hf_storage::{ChainStop, LaunchRow, Store};

pub fn stop_chain(context: &Context, store: &Store, spec: &StopSpec) -> Result<Response> {
    match &spec.mode {
        StopMode::Marker { cancelled } => store.put_chain_stop(
            &spec.chain_root,
            &ChainStop::Marker {
                cancelled_task_id: cancelled.clone(),
            },
            context.deadline(),
        )?,
        StopMode::Fault => {
            store.put_chain_stop(&spec.chain_root, &ChainStop::Fault, context.deadline())?
        }
        StopMode::Absent => store.remove_chain_stop(&spec.chain_root, context.deadline())?,
    }
    Ok(Response::Null)
}
pub fn add_launch(context: &Context, store: &Store, spec: &LaunchSpec) -> Result<Response> {
    store.put_launch_attempt(&row(spec), context.deadline())?;
    Ok(Response::Null)
}
pub fn add_invalid_launch_name(context: &Context, store: &Store, name: &str) -> Result<Response> {
    // A receipt named by a task identity is a launch, not an invalid name.
    if hf_domain::task_read::TaskId::new(name.to_owned()).is_ok() {
        return Err(crate::fail::Fail::new(
            "an invalid launch name must not be a task identity",
        ));
    }
    let row = LaunchRow {
        name: name.to_owned(),
        ..LaunchRow::default()
    };
    store.put_launch_attempt(&row, context.deadline())?;
    Ok(Response::Null)
}
/// How a receipt fares: it qualifies as a document or not, and the pending page refuses it
/// or not.
struct Verdict {
    document_qualifies: bool,
    /// The sanitized `(code, check)` of a refusal.
    refusal: Option<(String, String)>,
}
fn verdict(spec: &LaunchSpec) -> Verdict {
    let refused = |document_qualifies, code: &str, check: &str| Verdict {
        document_qualifies,
        refusal: Some((code.to_owned(), check.to_owned())),
    };
    match spec.refusal.as_deref() {
        Some("resource_names") => return refused(true, "field_shape", "resource_names"),
        Some(check) => return refused(false, check, check),
        None => {}
    }
    let names_present = spec.resources.as_ref().is_none_or(|r| r.names.is_some());
    let path_present = spec.worktree.as_ref().is_none_or(|w| w.path.is_some());
    if !names_present || !path_present {
        return refused(false, "field_shape", "field_shape");
    }
    match &spec.resources {
        Some(resources) if !names_valid(resources.names.as_deref().unwrap_or_default()) => {
            refused(true, "field_shape", "resource_names")
        }
        _ => Verdict {
            document_qualifies: true,
            refusal: None,
        },
    }
}
fn row(spec: &LaunchSpec) -> LaunchRow {
    let verdict = verdict(spec);
    let mut row = LaunchRow {
        name: spec.name.as_str().to_owned(),
        task_id: Some(spec.name.clone()),
        ..LaunchRow::default()
    };
    // A refused receipt keeps its status columns only when it still qualifies as a document
    // (a receipt the ordinary reads can read); its display columns are never kept.
    // `resource_names` refusals force a resources object, as the reference harness does.
    if verdict.document_qualifies {
        let forced = spec.refusal.as_deref() == Some("resource_names");
        row.state = Some(spec.state.clone());
        row.not_started_reason = spec.not_started_reason.clone();
        row.has_resources = Some(spec.resources.is_some() || forced);
        row.has_worktree = Some(spec.worktree.is_some());
    }
    match verdict.refusal {
        Some((code, check)) => {
            row.refusal_code = Some(code);
            row.refusal_check = Some(check);
        }
        None => {
            row.generation = Some(spec.generation.clone());
            row.title = spec.title.clone();
            row.backend = Some(spec.backend.clone());
            row.repo_path = spec.repo_path.clone();
            row.created_at = spec.created_at.clone();
            row.spawned_by = spec.lineage.spawned_by.clone();
            row.root_task_id = spec.lineage.root_task_id.clone();
            row.parent_task_id = spec.lineage.parent_task_id.clone();
            row.chain_root_task_id = spec.lineage.chain_root_task_id.clone();
            row.resource_names = spec.resources.as_ref().and_then(|r| r.names.clone());
        }
    }
    row
}
