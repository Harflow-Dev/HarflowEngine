//! Refusals of the interaction-events and statistics-page reads.
use super::read_errors::{ErrorDoc, ReadPath};
use hf_domain::ctl::caller::RefusalKind;

/// The refusal of `interaction-events` (one code, one message) or `statistics-page`.
pub(super) fn refusal(path: ReadPath, kind: RefusalKind, who: &str) -> ErrorDoc {
    if path == ReadPath::Interactions {
        return ErrorDoc::new("unauthorized", "Interaction events require a direct caller");
    }
    let code = match kind {
        RefusalKind::Agent => "agent_caller",
        RefusalKind::Undecidable => "caller_undecidable",
    };
    ErrorDoc::new(
        code,
        format!("Statistics require a verified direct caller: {who}"),
    )
}
