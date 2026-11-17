//! Display summary from an admitted root result; never execution authority.
use hf_application::task_runtime::{
    InvalidIntent, TerminalProjectionFacts, TerminalProjectionValidator,
};
use hf_domain::task_lifecycle::{LifecycleEvidence, ResultWitness};
use serde_json::Value;

pub fn successful_result_summary(line: &[u8]) -> Result<Option<String>, InvalidIntent> {
    let value: Value = serde_json::from_slice(line).map_err(|_| InvalidIntent)?;
    if value["type"] != "result"
        || value["subtype"] != "success"
        || value["is_error"] != false
        || !value["parent_tool_use_id"].is_null()
    {
        return Ok(None);
    }
    Ok(value["result"]
        .as_str()
        .filter(|s| s.len() <= 16384)
        .map(str::to_owned))
}

pub fn terminal_projection_with_summary(
    facts: &TerminalProjectionFacts<'_>,
    summary: Option<&str>,
) -> Result<(Vec<u8>, Vec<u8>), InvalidIntent> {
    let baseline = super::terminal_projection(facts)?;
    let Some(summary) =
        summary.filter(|_| facts.initial.status == "completed" && facts.exit_code == 0)
    else {
        return Ok(baseline);
    };
    if summary.len() > 16384 {
        return Ok(baseline);
    }
    let mut value: Value = serde_json::from_slice(&baseline.0).map_err(|_| InvalidIntent)?;
    value["summary"] = Value::String(summary.into());
    let bytes = serde_json::to_vec(&value).map_err(|_| InvalidIntent)?;
    if bytes.len() > 16384 {
        return Ok(baseline);
    }
    Ok((bytes.clone(), bytes))
}

pub struct CanonicalTerminalSummaryProjection<'a> {
    pub summary: &'a str,
    pub result: &'a ResultWitness,
}
impl TerminalProjectionValidator for CanonicalTerminalSummaryProjection<'_> {
    fn validate(
        &self,
        evidence: &LifecycleEvidence,
        facts: &TerminalProjectionFacts<'_>,
        header: &[u8],
        status: &[u8],
        event_body: &[u8],
        seq: u64,
    ) -> Result<(), InvalidIntent> {
        if evidence.observation.result.as_ref() != Some(self.result)
            || !self.result.success
            || facts.initial.status != "completed"
            || facts.exit_code != 0
        {
            return Err(InvalidIntent);
        }
        let (h, s) = terminal_projection_with_summary(facts, Some(self.summary))?;
        if h != header
            || s != status
            || super::task_finished_observed(facts, seq, &evidence.observation)? != event_body
        {
            return Err(InvalidIntent);
        }
        Ok(())
    }
}
