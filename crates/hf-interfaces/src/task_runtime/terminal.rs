//! Existing public baseline task and finished-event projections only.
use hf_application::task_runtime::{
    InvalidIntent, TerminalProjectionFacts, TerminalProjectionValidator,
};
use hf_domain::task_lifecycle::LifecycleEvidence;
use serde_json::{Value, json};
pub struct CanonicalTerminalProjection;
pub fn terminal_projection(
    f: &TerminalProjectionFacts<'_>,
) -> Result<(Vec<u8>, Vec<u8>), InvalidIntent> {
    let i = &f.initial;
    if !matches!(i.status, "completed" | "failed" | "cancelled") || !f.output_drained {
        return Err(InvalidIntent);
    }
    let bytes=serde_json::to_vec(&json!({"task_id":i.task.as_str(),"backend":i.backend,"repo_path":i.repo_path,"session_id":i.session_id,"status":i.status,"started_at":i.started_at,"title":i.title,"model":i.model,"continuation_of_task_id":i.continuation_of.map(|v|v.as_str()),"chain_root_task_id":i.chain_root,"exit_code":f.exit_code,"finished_at":f.finished_at,"output_drained":true})).map_err(|_|InvalidIntent)?;
    if bytes.len() > 16384 {
        return Err(InvalidIntent);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| InvalidIntent)?;
    super::projection::with_enforcement(value)
}
pub fn task_finished(f: &TerminalProjectionFacts<'_>, seq: u64) -> Result<Vec<u8>, InvalidIntent> {
    if seq > i64::MAX as u64 {
        return Err(InvalidIntent);
    }
    let mut event = json!({"seq":seq,"observed_at":f.finished_at,"source_ts":null,"raw_offset":null,"task_id":f.initial.task.as_str(),"kind":"task_finished","status":f.initial.status,"exit_code":f.exit_code,"summary":null,"observed":true});
    let object = event.as_object_mut().ok_or(InvalidIntent)?;
    object.insert("product".into(), Value::from("harflow"));
    object.insert("format".into(), Value::from("harflow.event"));
    object.insert("v".into(), Value::from(1));
    object.insert(
        "product_version".into(),
        Value::from(hf_domain::ctl::identity::PRODUCT_VERSION),
    );
    serde_json::to_vec(&event).map_err(|_| InvalidIntent)
}
/// Canonical summary field discloses durable normalized omissions without claiming raw losslessness.
pub fn task_finished_observed(
    f: &TerminalProjectionFacts<'_>,
    seq: u64,
    observation: &hf_domain::task_lifecycle::ExecutionObservation,
) -> Result<Vec<u8>, InvalidIntent> {
    let bytes = task_finished(f, seq)?;
    if observation.omitted_events == 0 {
        return Ok(bytes);
    }
    let mut value: Value = serde_json::from_slice(&bytes).map_err(|_| InvalidIntent)?;
    value["summary"] = Value::String(format!(
        "{} normalized events omitted.",
        observation.omitted_events
    ));
    serde_json::to_vec(&value).map_err(|_| InvalidIntent)
}
impl TerminalProjectionValidator for CanonicalTerminalProjection {
    fn validate(
        &self,
        evidence: &LifecycleEvidence,
        f: &TerminalProjectionFacts<'_>,
        header: &[u8],
        status: &[u8],
        event_body: &[u8],
        seq: u64,
    ) -> Result<(), InvalidIntent> {
        let (h, s) = terminal_projection(f)?;
        if h != header
            || s != status
            || task_finished_observed(f, seq, &evidence.observation)? != event_body
        {
            return Err(InvalidIntent);
        }
        Ok(())
    }
}
