//! Identity-free control request bytes; kernel peer admission belongs to platform/host.
use hf_application::task_runtime::InvalidIntent;
use hf_domain::{
    ownership::Id,
    task_lifecycle::LifecycleBinding,
    task_read::TaskId,
    task_runtime::{ControlKind, ControlRequest},
};
use serde_json::json;
pub fn encode_control(request: &ControlRequest) -> Result<Vec<u8>, InvalidIntent> {
    let mut out = b"HFTCANCEL\x01".to_vec();
    out.push(match request.kind {
        ControlKind::Cancel => 0,
    });
    let task = request.binding.task.as_str();
    out.push(task.len() as u8);
    out.extend_from_slice(task.as_bytes());
    for value in [
        request.binding.operation.as_str(),
        request.binding.attempt.as_str(),
        request.binding.generation.as_str(),
        request.request_id.as_str(),
    ] {
        out.extend_from_slice(value.as_bytes());
    }
    Ok(out)
}
pub fn decode_control(bytes: &[u8]) -> Result<ControlRequest, InvalidIntent> {
    if bytes.len() > 16384 || bytes.len() < 12 || &bytes[..10] != b"HFTCANCEL\x01" || bytes[10] != 0
    {
        return Err(InvalidIntent);
    }
    let length = usize::from(bytes[11]);
    let end = 12usize.checked_add(length).ok_or(InvalidIntent)?;
    if length == 0 || length > 64 || bytes.len() != end + 128 {
        return Err(InvalidIntent);
    }
    let text = |start: usize, len: usize| {
        std::str::from_utf8(&bytes[start..start + len])
            .map(str::to_owned)
            .map_err(|_| InvalidIntent)
    };
    let id = |offset| Id::new(text(end + offset, 32)?).map_err(|_| InvalidIntent);
    let request = ControlRequest {
        binding: LifecycleBinding {
            task: TaskId::new(text(12, length)?).map_err(|_| InvalidIntent)?,
            operation: id(0)?,
            attempt: id(32)?,
            generation: id(64)?,
        },
        request_id: id(96)?,
        kind: ControlKind::Cancel,
    };
    if encode_control(&request)? != bytes {
        return Err(InvalidIntent);
    }
    Ok(request)
}
/// Existing canonical result shape. Incomplete cascade coverage is explicit even for a
/// settled original child; this first-task subset never claims descendant convergence.
pub fn cancel_response(
    task: &TaskId,
    status: &str,
    observed_at: &str,
    original_settled: bool,
) -> Result<Vec<u8>, InvalidIntent> {
    if !matches!(status, "running" | "completed" | "failed" | "cancelled")
        || hf_domain::timestamp::parse_micros(observed_at).is_none()
        || original_settled && status == "running"
    {
        return Err(InvalidIntent);
    }
    let unresolved = if original_settled {
        Vec::new()
    } else {
        vec![task.as_str()]
    };
    let body = json!({"product":"harflow","format":"harflow.ctl","v":1,"product_version":hf_domain::ctl::identity::PRODUCT_VERSION,"result":{"task_id":task.as_str(),"status":status,"cascade":{"cancelled_descendants":[],"cascade_incomplete":true,"history_incomplete":true,"not_recorded":[],"not_signalled":[],"observation":{"coverage":"incomplete","items":[{"task_id":task.as_str(),"state":if original_settled {"stopped"}else{"unknown"},"basis":if original_settled {"observed_settlement"}else{"insufficient_evidence"},"status":status}],"observed_at":observed_at,"scope":"recorded_tasks_and_pending_launches","truncated":false},"owner_still_settling":[],"refused_tasks":[],"rounds":1,"sigkill_survivors":[],"unconverged":unresolved}}});
    serde_json::to_vec(&body).map_err(|_| InvalidIntent)
}
