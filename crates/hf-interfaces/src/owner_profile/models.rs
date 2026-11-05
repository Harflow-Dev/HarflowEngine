use super::{Checked, fields::*, invalid, unsupported};
use hf_application::owner_policy::evaluate_permission;
use hf_domain::owner_policy::*;
use serde_json::{Value, json};
pub(super) fn policy(d: &Value) -> Checked<Value> {
    if !integer(&d["profile_version"], 0, u64::MAX)
        || d.get("background_requirement")
            .is_some_and(|v| !v.is_string())
        || d.get("permission_mode").is_some_and(|v| !v.is_string())
        || d.get("background_idle_seconds")
            .is_some_and(|v| !v.as_f64().is_some_and(|n| n > 0.0 && n <= 21600.0))
    {
        return Err(invalid("fields"));
    }
    let requirement = d
        .get("background_requirement")
        .and_then(Value::as_str)
        .unwrap_or("required");
    let mode = d
        .get("permission_mode")
        .and_then(Value::as_str)
        .unwrap_or("os_enforced");
    if d["profile_version"].as_u64() != Some(1)
        || !["required", "incidental"].contains(&requirement)
        || !["os_enforced", "tool_level"].contains(&mode)
    {
        return Err(unsupported("enum"));
    }
    Ok(
        json!({"parse":"valid","decision":"accept","background_requirement":requirement,
        "permission_mode":mode,"background_idle_seconds":d.get("background_idle_seconds").cloned().unwrap_or(json!(600)),
        "tool_level_opt_in":mode=="tool_level"}),
    )
}
pub(super) fn permission(d: &Value) -> Checked<Value> {
    shape(
        d,
        &[
            "requested_mode",
            "established_mode",
            "qualification",
            "binary_match",
            "configuration_match",
            "grants_match",
            "protected_paths_match",
        ],
        &["tool_level_opt_in"],
    )?;
    enums(
        d,
        &[
            ("requested_mode", "os_enforced tool_level"),
            ("established_mode", "os_enforced tool_level unknown"),
            ("qualification", "passed failed unknown unmeasured"),
            ("binary_match", "proven unknown mismatch"),
            ("configuration_match", "proven unknown mismatch"),
            ("grants_match", "proven unknown mismatch"),
            ("protected_paths_match", "proven unknown mismatch"),
        ],
    )?;
    let value = PermissionObservation {
        requested_mode: mode(text(d, "requested_mode")),
        established_mode: if text(d, "established_mode") == "unknown" {
            None
        } else {
            Some(mode(text(d, "established_mode")))
        },
        tool_level_opt_in: d["tool_level_opt_in"].as_bool().unwrap_or(false),
        qualification: match text(d, "qualification") {
            "passed" => QualificationState::Passed,
            "failed" => QualificationState::Failed,
            "unmeasured" => QualificationState::Unmeasured,
            _ => QualificationState::Unknown,
        },
        matches: [
            "binary_match",
            "configuration_match",
            "grants_match",
            "protected_paths_match",
        ]
        .map(|key| match text(d, key) {
            "proven" => MatchState::Proven,
            "mismatch" => MatchState::Mismatch,
            _ => MatchState::Unknown,
        }),
    };
    let result = evaluate_permission(&value);
    Ok(
        json!({"parse":"valid","decision":if result.permitted {"admit"} else {"refuse"},"reason":result.reason}),
    )
}
fn mode(s: &str) -> PermissionMode {
    if s == "tool_level" {
        PermissionMode::ToolLevel
    } else {
        PermissionMode::OsEnforced
    }
}
pub(super) fn steering(d: &Value) -> Checked<Value> {
    shape(d, &["claimed_proof", "requested_action"], &[])?;
    enums(
        d,
        &[
            (
                "claimed_proof",
                "agent_result storage_ack observer_attachment",
            ),
            (
                "requested_action",
                "withdraw retire replace release control",
            ),
        ],
    )?;
    Ok(json!({"parse":"valid","decision":"refuse","reason":"not_steering_authority"}))
}
