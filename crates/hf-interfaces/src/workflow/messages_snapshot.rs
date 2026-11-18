//! Text for pure snapshot and invocation-path resource refusals.
use super::messages::failure_message;
use hf_application::workflow::snapshot::SnapshotFailure;
use hf_application::workflow::snapshot_scopes::ResourceScopeFailure;

pub fn snapshot_failure_message(failure: &SnapshotFailure) -> String {
    match failure {
        SnapshotFailure::Validation(error) => failure_message(error),
        SnapshotFailure::SourceNameMismatch => {
            "Authorized workflow snapshot name mismatch".to_string()
        }
        SnapshotFailure::PinHashMismatch => {
            "Pinned workflow snapshot does not match its content hash".to_string()
        }
        SnapshotFailure::RootMismatch => {
            "Resolved dependency tree does not match the starting definition".to_string()
        }
        SnapshotFailure::Scope(error) => resource_scope_failure_message(error),
    }
}

pub fn resource_scope_failure_message(failure: &ResourceScopeFailure) -> String {
    match failure {
        ResourceScopeFailure::CycleOrDepth => {
            "Workflow resource validation found a cyclic or too-deep dependency path".to_string()
        }
        ResourceScopeFailure::MissingDependency { workflow_id } => {
            format!("Workflow resource validation found a missing pinned dependency: {workflow_id}")
        }
        ResourceScopeFailure::WorkflowExceeds {
            workflow,
            resources,
            enclosing,
        } => format!(
            "Workflow {workflow} resources {} exceed enclosing declaration {enclosing}",
            resources.join(", ")
        ),
        ResourceScopeFailure::NodeExceeds {
            workflow,
            node_id,
            resources,
            enclosing,
        } => format!(
            "Workflow {workflow} node {node_id} resources {} exceed enclosing declaration {enclosing}",
            resources.join(", ")
        ),
    }
}
