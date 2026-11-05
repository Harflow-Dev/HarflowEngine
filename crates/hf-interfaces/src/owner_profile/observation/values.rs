use hf_domain::owner_policy::*;
pub(super) fn value_agent_result(s: &str) -> ResultState {
    match s {
        "unknown" => ResultState::Unknown,
        "succeeded" => ResultState::Succeeded,
        _ => ResultState::Failed,
    }
}
pub(super) fn value_result_binding(s: &str) -> ResultBinding {
    match s {
        "matched" => ResultBinding::Matched,
        "mismatched" => ResultBinding::Mismatched,
        _ => ResultBinding::Unknown,
    }
}
pub(super) fn value_requirement(s: &str) -> Requirement {
    match s {
        "required" => Requirement::Required,
        "incidental" => Requirement::Incidental,
        _ => Requirement::Unknown,
    }
}
pub(super) fn value_obligations(s: &str) -> ObligationState {
    match s {
        "pending" => ObligationState::Pending,
        "satisfied" => ObligationState::Satisfied,
        "failed" => ObligationState::Failed,
        _ => ObligationState::Unknown,
    }
}
pub(super) fn value_inventory(s: &str) -> InventoryState {
    match s {
        "complete" => InventoryState::Complete,
        "partial" => InventoryState::Partial,
        _ => InventoryState::Unknown,
    }
}
pub(super) fn value_exit(s: &str) -> ExitState {
    match s {
        "unobserved" => ExitState::Unobserved,
        "zero" => ExitState::Zero,
        "nonzero" => ExitState::Nonzero,
        _ => ExitState::Signal,
    }
}
pub(super) fn value_stdout(s: &str) -> PipeState {
    match s {
        "eof" => PipeState::Eof,
        "open" => PipeState::Open,
        "unknown" => PipeState::Unknown,
        _ => PipeState::Error,
    }
}
pub(super) fn value_stderr(s: &str) -> PipeState {
    match s {
        "eof" => PipeState::Eof,
        "open" => PipeState::Open,
        "unknown" => PipeState::Unknown,
        _ => PipeState::Error,
    }
}
pub(super) fn value_input(s: &str) -> InputState {
    match s {
        "reconciled" => InputState::Reconciled,
        "pending" => InputState::Pending,
        "unknown" => InputState::Unknown,
        _ => InputState::Error,
    }
}
pub(super) fn value_capture(s: &str) -> CaptureState {
    match s {
        "lossless" => CaptureState::Lossless,
        "known_loss" => CaptureState::KnownLoss,
        "unknown" => CaptureState::Unknown,
        _ => CaptureState::Error,
    }
}
pub(super) fn value_watermark(s: &str) -> WatermarkState {
    match s {
        "complete" => WatermarkState::Complete,
        "gap" => WatermarkState::Gap,
        _ => WatermarkState::Unknown,
    }
}
pub(super) fn value_containment(s: &str) -> ContainmentState {
    match s {
        "complete" => ContainmentState::Complete,
        "incomplete" => ContainmentState::Incomplete,
        _ => ContainmentState::Unknown,
    }
}
pub(super) fn value_writer(s: &str) -> WriterState {
    match s {
        "settled" => WriterState::Settled,
        "live" => WriterState::Live,
        _ => WriterState::Unknown,
    }
}
pub(super) fn value_claims(s: &str) -> ClaimsState {
    match s {
        "reconciled" => ClaimsState::Reconciled,
        "pending" => ClaimsState::Pending,
        _ => ClaimsState::Unknown,
    }
}
pub(super) fn value_execution(s: &str) -> ExecutionState {
    match s {
        "healthy" => ExecutionState::Healthy,
        "failed" => ExecutionState::Failed,
        "cancelled" => ExecutionState::Cancelled,
        _ => ExecutionState::Unknown,
    }
}
