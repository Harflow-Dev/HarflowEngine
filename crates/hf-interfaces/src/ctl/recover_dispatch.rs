//! Public recovery documents derived from qualified application outcomes.
use super::{
    dispatch::{CtlServices, Outcome},
    render,
};
use hf_application::ctl::state_recover::{
    flow::{RecoveryError, RecoveryFailure, RecoveryPhase},
    journal::RecoveryResult,
};
use hf_domain::ctl::{
    identity::{CTL_FORMAT, PRODUCT_VERSION},
    state_root::StateRoot,
};
use serde_json::{Value, json};
pub fn render_result(result: &RecoveryResult) -> Value {
    json!({"state_root": result.state_root, "engine_version": PRODUCT_VERSION,
        "operation_id": result.operation_id, "backup_id": result.operation_id,
        "backup_path": result.backup_path, "phase": "completed", "status": "completed", "recovery_supported": true})
}
pub fn render_failure(root: &StateRoot, failure: &RecoveryFailure) -> Value {
    let (code, reason, message, supported) = match failure.error {
        RecoveryError::StaleConfirmation => (
            "state_recovery_stale",
            "stale_confirmation",
            "Confirmation does not match this engine, root, or operation",
            true,
        ),
        RecoveryError::ActiveOwner => (
            "state_busy",
            "active_owner",
            "Cannot recover while a state owner is active",
            true,
        ),
        RecoveryError::AmbiguousOwner => (
            "state_busy",
            "ambiguous_owner",
            "Known owner evidence cannot be qualified",
            true,
        ),
        RecoveryError::PythonState => (
            "state_recovery_unsafe",
            "unsafe_root",
            "This root contains Python engine state; use the Python engine or select a fresh state root",
            false,
        ),
        RecoveryError::UnsafeRoot => (
            "state_recovery_unsafe",
            "unsafe_root",
            "Recovery requires a dedicated state directory",
            false,
        ),
        RecoveryError::UnsupportedLayout => (
            "state_recovery_unsafe",
            "phase_layout_recovery_unsupported",
            "Destructive recovery of phase-enabled layout 2 is unsupported; resolve every lease, stop the slot service, and select a fresh state root. No backup was created",
            false,
        ),
        RecoveryError::BackupConflict => (
            "state_recovery_unsafe",
            "backup_conflict",
            "Recovery backup cannot be qualified",
            false,
        ),
        RecoveryError::InfrastructureUnavailable => (
            "state_recovery_failed",
            "infrastructure_unavailable",
            "State recovery did not complete",
            true,
        ),
        RecoveryError::RecoveryInterrupted => (
            "state_recovery_failed",
            "recovery_interrupted",
            "State recovery did not complete",
            true,
        ),
    };
    let mut data = json!({"state_root": root.as_str(), "engine_version": PRODUCT_VERSION,
        "reason": reason, "recovery_supported": supported});
    if let Some(progress) = &failure.progress {
        data["operation_id"] = json!(progress.operation_id);
        data["backup_id"] = json!(progress.operation_id);
        data["backup_path"] = json!(progress.backup_path);
        data["phase"] = json!(match progress.phase {
            RecoveryPhase::Planned => "planned",
            RecoveryPhase::Prepared => "prepared",
            RecoveryPhase::BackedUp => "backed_up",
            RecoveryPhase::Committed => "committed",
            RecoveryPhase::Completed => "completed",
        });
        data["recovery_supported"] = json!(true);
    }
    json!({"code": code, "message": message, "data": data})
}
pub(super) fn run(
    root: &StateRoot,
    operation_id: &str,
    token: &str,
    json: bool,
    services: &dyn CtlServices,
) -> Outcome {
    match services.recover(root, operation_id, token) {
        Ok(result) => {
            let result = render_result(&result);
            Outcome {
                stdout: if json {
                    render::line(&render::stamp(CTL_FORMAT, json!({"result": result})))
                } else {
                    render::pretty(&result)
                },
                stderr: String::new(),
                code: 0,
            }
        }
        Err(failure) => {
            let error = render_failure(root, &failure);
            Outcome {
                stderr: format!(
                    "harflow-ctl: error: {}\n",
                    error["message"]
                        .as_str()
                        .unwrap_or("State recovery did not complete")
                ),
                stdout: if json {
                    render::line(&render::stamp(CTL_FORMAT, json!({"error": error})))
                } else {
                    String::new()
                },
                code: 1,
            }
        }
    }
}
