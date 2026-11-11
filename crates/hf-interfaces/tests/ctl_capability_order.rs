//! The advertised capability lists keep the canonical order of the reference engine.
use hf_domain::ctl::identity::capabilities;
/// `wire_contracts.CAPABILITIES` of HarflowMCP 0.0.13 (`0ac7e3c`).
const PYTHON_ORDER: [&str; 34] = [
    "task-pending-launch.v1",
    "conversation-delivery.v1",
    "workflow-steering-replan.v1",
    "workflow-steering-worker.v1",
    "workflow-steering-window.v1",
    "workflow-steering-read.v1",
    "workflow-steering-builder.v1",
    "workflow-steering-orchestrator.v1",
    "workflow-shared-interactions.v1",
    "interaction-events.v1",
    "workflow-attention-checkpoints.v1",
    "phase-slots.v1",
    "ctl.v1",
    "setup.v1",
    "events.v1",
    "version-info.v1",
    "state-recovery.v1",
    "caller-approvals.monitor-respond.v1",
    "statistics-page.v1",
    "backend-qualification.v1",
    "messaging.v1",
    "limit-hits.v1",
    "workflow-limit-hits.v1",
    "task-event-page.v1",
    "task-prompt.v1",
    "task-successors.v1",
    "workflow-live-activity.v1",
    "statistics-cost-model.v1",
    "task-changes.v1",
    "task-list-watch.v1",
    "task-continue.v1",
    "task-artifacts.v1",
    "task-continuation-stop.v1",
    "task-cancel-cascade.v1",
];
fn is_subsequence(emitted: &[&str]) -> bool {
    let mut python = PYTHON_ORDER.iter();
    emitted
        .iter()
        .all(|name| python.by_ref().any(|candidate| candidate == name))
}
#[test]
fn emitted_lists_keep_the_canonical_order_in_both_availability_states() {
    let with_store = capabilities(true);
    let without_store = capabilities(false);
    assert!(is_subsequence(&with_store));
    assert!(is_subsequence(&without_store));
    assert_eq!(without_store, ["ctl.v1", "version-info.v1"]);
    assert_eq!(
        with_store,
        [
            "interaction-events.v1",
            "ctl.v1",
            "version-info.v1",
            "statistics-page.v1",
            "task-event-page.v1",
            "task-prompt.v1",
            "task-successors.v1",
            "task-artifacts.v1",
            "task-continuation-stop.v1",
        ]
    );
}
