use super::{Checked, fields::*, invalid};
use hf_application::owner_policy::evaluate_exchange;
use hf_domain::owner_policy::*;
use serde_json::{Value, json};
pub(super) fn classify(d: &Value, binding: &Value) -> Checked<Value> {
    shape(d, &["send_state", "deadline"], &["poisoned"])?;
    let p = &d["pending"];
    let a = &d["ack"];
    let pending_identity = identity(p)?;
    let ack_identity = identity(a)?;
    shape(p, &["command"], &[])?;
    shape(a, &["state"], &[])?;
    if !integer(&p["expected_revision"], 0, u64::MAX)
        || !integer(&p["payload_bytes"], 0, 16384)
        || a.get("revision").is_none()
        || (!a["revision"].is_null() && !integer(&a["revision"], 0, u64::MAX))
        || (text(a, "state") == "committed" && a["revision"].is_null())
        || (text(p, "command") != "append" && p["payload_bytes"].as_u64() != Some(0))
    {
        return Err(invalid("fields"));
    }
    enums(
        d,
        &[
            ("send_state", "complete partial unknown"),
            ("deadline", "current expired"),
        ],
    )?;
    enums(
        p,
        &[(
            "command",
            "bootstrap claim record_child create_raw accept capture_begin append known_loss finish close",
        )],
    )?;
    enums(a, &[("state", "committed refused unknown")])?;
    let value = ExchangeObservation {
        pending: PendingExchange {
            identity: pending_identity,
            command: command(text(p, "command")),
            expected_revision: p["expected_revision"].as_u64().unwrap_or(0),
            payload_bytes: p["payload_bytes"].as_u64().unwrap_or(0) as usize,
        },
        ack: AckExchange {
            identity: ack_identity,
            revision: a["revision"].as_u64(),
            state: match text(a, "state") {
                "committed" => AckState::Committed,
                "refused" => AckState::Refused,
                _ => AckState::Unknown,
            },
        },
        send_state: match text(d, "send_state") {
            "complete" => SendState::Complete,
            "partial" => SendState::Partial,
            _ => SendState::Unknown,
        },
        deadline: if text(d, "deadline") == "current" {
            DeadlineState::Current
        } else {
            DeadlineState::Expired
        },
        poisoned: d["poisoned"].as_bool().unwrap_or(false),
    };
    let r = evaluate_exchange(
        &value,
        &id(&binding["operation"])?,
        &id(&binding["generation"])?,
        &id(&binding["attempt"])?,
    );
    Ok(
        json!({"parse":"valid","decision":if r.permitted {"advance"} else {"retain"},"reason":r.reason}),
    )
}
fn identity(v: &Value) -> Checked<ExchangeIdentity> {
    if !v.is_object() || !integer(&v["sequence"], 1, u64::MAX) {
        return Err(invalid("fields"));
    }
    Ok(ExchangeIdentity {
        sequence: v["sequence"].as_u64().unwrap_or(0),
        operation: id(&v["operation"])?,
        generation: id(&v["generation"])?,
        attempt: id(&v["attempt"])?,
        channel_nonce: id(&v["channel_nonce"])?,
        writer: process(&v["writer"]).ok_or(invalid("fields"))?,
        binding_digest: digest(&v["binding_digest"])?,
        command_digest: digest(&v["command_digest"])?,
    })
}
fn command(s: &str) -> StorageModelCommand {
    match s {
        "bootstrap" => StorageModelCommand::Bootstrap,
        "claim" => StorageModelCommand::Claim,
        "record_child" => StorageModelCommand::RecordChild,
        "create_raw" => StorageModelCommand::CreateRaw,
        "accept" => StorageModelCommand::Accept,
        "capture_begin" => StorageModelCommand::CaptureBegin,
        "append" => StorageModelCommand::Append,
        "known_loss" => StorageModelCommand::KnownLoss,
        "finish" => StorageModelCommand::Finish,
        _ => StorageModelCommand::Close,
    }
}
