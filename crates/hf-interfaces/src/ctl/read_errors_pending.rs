//! Error documents of the pending launch reads: every one reports unknown coverage.
use super::read_errors::ErrorDoc;
use hf_application::ctl::task_reads::PendingFailure;
use serde_json::json;

pub(super) fn doc(failure: &PendingFailure) -> ErrorDoc {
    match failure {
        PendingFailure::Marker => {
            let mut doc = ErrorDoc::new(
                "agent_caller",
                "Pending launch reads require a verified direct caller.",
            );
            doc.data = Some(json!({"coverage": "unknown", "check": "direct_caller_required"}));
            doc
        }
        PendingFailure::Unavailable => {
            let mut doc = ErrorDoc::new(
                "pending_observation_unavailable",
                "Pending launch coverage is unknown; selected state is unavailable.",
            );
            doc.data = Some(json!({"coverage": "unknown"}));
            doc
        }
        PendingFailure::Check { code, check } => {
            let message =
                format!("Pending launch observation unavailable ({check}); coverage is unknown.");
            let mut doc = ErrorDoc::new(code, message);
            doc.data = Some(json!({"coverage": "unknown", "check": check}));
            doc
        }
    }
}
