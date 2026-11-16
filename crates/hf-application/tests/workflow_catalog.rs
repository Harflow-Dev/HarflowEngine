//! The table catalog refuses an allowed_commands lookup it has no recorded outcome for.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_support;

use hf_application::workflow::ports::BackendCatalog;
use hf_domain::workflow::vocab::Freedom;
use workflow_support::TableCatalog;

#[test]
fn a_missing_row_is_a_refusal_not_an_acceptance() {
    let patterns = vec!["never-recorded".to_string()];
    let message = TableCatalog
        .check_allowed_commands("claude", &patterns, Freedom::Publish)
        .unwrap_err();
    assert!(
        message.contains("no recorded allowed_commands outcome"),
        "{message}"
    );
    assert!(message.contains("claude"), "{message}");
    assert!(message.contains("never-recorded"), "{message}");
}

#[test]
fn a_missing_backend_or_freedom_is_also_refused() {
    let patterns = vec!["git status".to_string()];
    assert!(
        TableCatalog
            .check_allowed_commands("nobackend", &patterns, Freedom::Publish)
            .unwrap_err()
            .contains("no recorded allowed_commands outcome")
    );
    assert!(
        TableCatalog
            .check_allowed_commands("claude", &[], Freedom::ReadOnly)
            .is_ok(),
        "the empty pattern list has a recorded acceptance"
    );
}

#[test]
fn recorded_rows_still_decide() {
    let status = vec!["git status".to_string()];
    assert!(
        TableCatalog
            .check_allowed_commands("antigravity", &status, Freedom::Publish)
            .unwrap_err()
            .contains("not qualified for the antigravity backend")
    );
}
