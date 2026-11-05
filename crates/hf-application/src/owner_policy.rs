//! Pure candidate-model decisions. These values never confer execution/control authority.
use hf_domain::owner_policy::*;
use hf_domain::ownership::Id;

pub fn evaluate_permission(value: &PermissionObservation) -> ModelDecision {
    let reason = if value.requested_mode == PermissionMode::ToolLevel && !value.tool_level_opt_in {
        "opt_in_required"
    } else if value.matches.contains(&MatchState::Mismatch) {
        "qualification_mismatch"
    } else if value.qualification != QualificationState::Passed
        || value.matches.contains(&MatchState::Unknown)
    {
        "unqualified"
    } else if value.established_mode != Some(value.requested_mode) {
        "mode_mismatch"
    } else {
        "qualified"
    };
    ModelDecision {
        permitted: reason == "qualified",
        reason,
    }
}

pub fn evaluate_observation(value: &RuntimeObservation) -> ObservationDecision {
    let result = display_result(value);
    let (workflow, reason) = workflow(value, result);
    let release = if workflow != WorkflowState::Blocked
        && value.containment == ContainmentState::Complete
        && value.claims == ClaimsState::Reconciled
        && value.inventory == InventoryState::Complete
        && matches!(
            value.obligations,
            ObligationState::Satisfied | ObligationState::Failed
        ) {
        ReleaseEvidence::EvidenceComplete
    } else {
        ReleaseEvidence::Blocked
    };
    ObservationDecision {
        result,
        workflow,
        release,
        reason,
    }
}

fn display_result(value: &RuntimeObservation) -> ResultState {
    if value.sticky_failure {
        ResultState::Failed
    } else if value.result_binding != ResultBinding::Matched {
        ResultState::Unknown
    } else if value.agent_result == ResultState::Failed {
        ResultState::Failed
    } else if value.turn_open {
        ResultState::Unknown
    } else {
        value.agent_result
    }
}

fn workflow(value: &RuntimeObservation, result: ResultState) -> (WorkflowState, &'static str) {
    let unsettled = value.exit == ExitState::Unobserved
        || value.stdout != PipeState::Eof
        || value.stderr != PipeState::Eof
        || value.input != InputState::Reconciled
        || value.capture != CaptureState::Lossless
        || value.watermark != WatermarkState::Complete
        || value.writer != WriterState::Settled
        || value.turn_open;
    let required = value.requirement != Requirement::Incidental;
    if unsettled {
        (WorkflowState::Blocked, "unsettled")
    } else if value.inventory != InventoryState::Complete
        || value.obligations == ObligationState::Unknown
        || (required && value.obligations == ObligationState::Pending)
    {
        (WorkflowState::Blocked, "obligations")
    } else if value.execution == ExecutionState::Unknown {
        (WorkflowState::Blocked, "execution_unknown")
    } else if value.execution == ExecutionState::Cancelled {
        (WorkflowState::Cancelled, "cancelled")
    } else if value.execution == ExecutionState::Failed
        || matches!(value.exit, ExitState::Nonzero | ExitState::Signal)
    {
        (WorkflowState::Failed, "execution_failed")
    } else if result == ResultState::Failed {
        (WorkflowState::Failed, "result_failed")
    } else if required && value.obligations == ObligationState::Failed {
        (WorkflowState::Failed, "required_obligation_failed")
    } else if result == ResultState::Succeeded {
        (WorkflowState::Succeeded, "succeeded")
    } else {
        (WorkflowState::Blocked, "result_unknown")
    }
}

pub fn evaluate_exchange(
    value: &ExchangeObservation,
    operation: &Id,
    generation: &Id,
    attempt: &Id,
) -> ModelDecision {
    let pending = &value.pending;
    let ack = &value.ack;
    let reason = if value.poisoned {
        "poisoned"
    } else if value.send_state != SendState::Complete {
        "uncertain_send"
    } else if value.deadline == DeadlineState::Expired {
        "late_ack"
    } else if pending.identity != ack.identity
        || &pending.identity.operation != operation
        || &pending.identity.generation != generation
        || &pending.identity.attempt != attempt
    {
        "identity_mismatch"
    } else if ack.state == AckState::Unknown {
        "commit_unknown"
    } else if ack.state == AckState::Refused {
        "storage_refusal"
    } else if expected_revision(pending).is_none() || expected_revision(pending) != ack.revision {
        "revision_mismatch"
    } else {
        "acknowledged"
    };
    ModelDecision {
        permitted: reason == "acknowledged",
        reason,
    }
}

fn expected_revision(value: &PendingExchange) -> Option<u64> {
    match value.command {
        StorageModelCommand::Claim
        | StorageModelCommand::RecordChild
        | StorageModelCommand::Accept
        | StorageModelCommand::Append
        | StorageModelCommand::KnownLoss
        | StorageModelCommand::Finish => value.expected_revision.checked_add(1),
        _ => Some(value.expected_revision),
    }
}
