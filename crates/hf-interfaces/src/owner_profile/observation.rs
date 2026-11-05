use super::{Checked, fields::*};
use hf_application::owner_policy::evaluate_observation;
use hf_domain::owner_policy::*;
use serde_json::{Value, json};
mod values;
pub(super) fn classify(d: &Value) -> Checked<Value> {
    validate(d)?;
    let value = RuntimeObservation {
        agent_result: values::value_agent_result(text(d, "agent_result")),
        result_binding: values::value_result_binding(text(d, "result_binding")),
        requirement: values::value_requirement(text(d, "requirement")),
        obligations: values::value_obligations(text(d, "obligations")),
        inventory: values::value_inventory(text(d, "inventory")),
        exit: values::value_exit(text(d, "exit")),
        stdout: values::value_stdout(text(d, "stdout")),
        stderr: values::value_stderr(text(d, "stderr")),
        input: values::value_input(text(d, "input")),
        capture: values::value_capture(text(d, "capture")),
        watermark: values::value_watermark(text(d, "watermark")),
        containment: values::value_containment(text(d, "containment")),
        writer: values::value_writer(text(d, "writer")),
        claims: values::value_claims(text(d, "claims")),
        execution: values::value_execution(text(d, "execution")),
        sticky_failure: d["sticky_failure"].as_bool().unwrap_or(false),
        turn_open: d["turn_open"].as_bool().unwrap_or(false),
    };
    let r = evaluate_observation(&value);
    let result = match r.result {
        ResultState::Unknown => "unknown",
        ResultState::Succeeded => "succeeded",
        ResultState::Failed => "failed",
    };
    let workflow = match r.workflow {
        WorkflowState::Blocked => "blocked",
        WorkflowState::Succeeded => "succeeded",
        WorkflowState::Failed => "failed",
        WorkflowState::Cancelled => "cancelled",
    };
    let release = match r.release {
        ReleaseEvidence::Blocked => "blocked",
        ReleaseEvidence::EvidenceComplete => "evidence_complete",
    };
    Ok(
        json!({"parse":"valid","result":result,"workflow":workflow,"release":release,"reason":r.reason}),
    )
}

fn validate(d: &Value) -> Checked<()> {
    shape(
        d,
        &[
            "agent_result",
            "result_binding",
            "requirement",
            "obligations",
            "inventory",
            "exit",
            "stdout",
            "stderr",
            "input",
            "capture",
            "watermark",
            "containment",
            "writer",
            "claims",
            "execution",
        ],
        &["sticky_failure", "turn_open"],
    )?;
    enums(
        d,
        &[
            ("agent_result", "unknown succeeded failed"),
            ("result_binding", "matched mismatched unknown"),
            ("requirement", "required incidental unknown"),
            ("obligations", "pending satisfied failed unknown"),
            ("inventory", "complete partial unknown"),
            ("exit", "unobserved zero nonzero signal"),
            ("stdout", "eof open unknown error"),
            ("stderr", "eof open unknown error"),
            ("input", "reconciled pending unknown error"),
            ("capture", "lossless known_loss unknown error"),
            ("watermark", "complete gap unknown"),
            ("containment", "complete incomplete unknown"),
            ("writer", "settled live unknown"),
            ("claims", "reconciled pending unknown"),
            ("execution", "healthy failed cancelled unknown"),
        ],
    )?;
    Ok(())
}
