//! The text of graph and Git refusals and of stored-document reasons.
use hf_domain::workflow::json::Json;
use hf_domain::workflow::refusal::DocumentReason;
use hf_domain::workflow::rule_graph::GraphRule;
use hf_domain::workflow::rule_shape::GitRule;

use super::messages::{Args, NAME_RULE};

pub(super) fn graph_message(rule: GraphRule, a: &Args<'_>) -> String {
    let (first, second) = (a.text(0), a.text(1));
    match rule {
        GraphRule::UnreachableFromStart => "All nodes must be reachable from Start".to_string(),
        GraphRule::AmbiguousLoop => {
            "Ambiguous loop: cycles must return to a step that dominates the retrying step"
                .to_string()
        }
        GraphRule::NotForwardReachable => {
            "All nodes must be forward-reachable from Start".to_string()
        }
        GraphRule::NoForwardPathToEnd => {
            format!(
                "Every node must have a forward path to End: {}",
                a.joined(0)
            )
        }
        GraphRule::ParallelUseBoundaries => {
            "Use Parallel start and Parallel end instead of Join".to_string()
        }
        GraphRule::ParallelDuplicateBoundary => {
            format!("Duplicate {first} for parallel group {second}")
        }
        GraphRule::ParallelGroupIncomplete => {
            format!("Parallel group {first} requires matching start and end")
        }
        GraphRule::ParallelStartBranches => format!(
            "Parallel start {first} requires at least two forward branches and no retry outputs"
        ),
        GraphRule::ParallelBranchMissesEnd => format!(
            "Every branch from Parallel start {first} must reach matching Parallel end {second}"
        ),
        GraphRule::ParallelEmptyBranch => {
            format!("Parallel start {first} cannot have an empty branch")
        }
        GraphRule::ParallelOverlap => {
            format!("Parallel group {first} has crossing or overlapping sibling branches")
        }
        GraphRule::ParallelReachesEnd => {
            format!("Parallel group {first} reaches workflow End before its matching end")
        }
        GraphRule::ParallelImproperNesting => {
            format!("Parallel group {first} contains an improperly nested boundary")
        }
        GraphRule::ParallelExternalEntry => {
            format!("External entry into parallel branch {first}: {second}")
        }
        GraphRule::ParallelEscape => {
            format!("Connection escapes parallel branch {first}: {second}")
        }
        GraphRule::ParallelRetryEntry => {
            format!("Retry cannot enter a closed parallel branch: {first}")
        }
        GraphRule::ParallelEndExternalEntry => format!("External entry into Parallel end {first}"),
        GraphRule::LegacyNoConvergence => {
            format!("Parallel branches from {first} must converge at one common node before ending")
        }
        GraphRule::LegacyCrossBranch => format!(
            "Cross-branch connection at split {first}: its legacy Join follows an earlier convergence; connect branches directly to their first shared node"
        ),
        GraphRule::JoinOutsideSplit => format!("Join {first} is outside its matching split"),
        GraphRule::SelectionJoinAmbiguous => {
            "Selected parallel paths must converge at one common node".to_string()
        }
        GraphRule::OptionalUnsafe => format!(
            "Optional nodes need a safe parallel branch with a required sibling and no required successor before convergence: {}",
            a.joined(0)
        ),
    }
}

pub(super) fn git_message(rule: GitRule, a: &Args<'_>) -> String {
    let text = match rule {
        GitRule::LiteralName => format!(
            "{} must be a literal name of letters, digits, '_', '-' and '/', starting with a letter or digit, with no empty, '.' or '..' component, and no options, refspecs or revision expressions",
            a.text(1)
        ),
        GitRule::BaseForm => "base must be <remote>/<branch>, for example origin/develop; the first component is the remote and the rest is the branch".to_string(),
        GitRule::FetchForm => "fetch must be true or a list of remote names".to_string(),
        GitRule::FetchEmpty => "fetch must name at least one remote, or be true for the default remote".to_string(),
        GitRule::FetchRequiredNotBool => "fetch_required must be a boolean".to_string(),
        GitRule::FetchRequiredNeedsFetch => "fetch_required needs fetch: true or a list of remote names".to_string(),
        GitRule::BranchBaseTogether => "branch and base are set together; pass both, for example branch='feature/x' and base='origin/develop'".to_string(),
        GitRule::BranchContinuation => "continuations reuse the branch prepared at dispatch and do not accept branch or base".to_string(),
        GitRule::BranchReadOnly => "branch preparation is unavailable at read_only; use write_in_repo or above, or pass fetch without branch".to_string(),
        GitRule::BranchWorktree => "branch preparation and worktree create different checkouts; pass one of them".to_string(),
    };
    match a.0.first() {
        Some(Json::Str(prefix)) => format!("{prefix}: {text}"),
        _ => text,
    }
}

/// The reason of a stored-document refusal.
pub fn reason_message(reason: &DocumentReason) -> String {
    match reason {
        DocumentReason::ForeignFormat => {
            "Missing or foreign workflow definition product/format".to_string()
        }
        DocumentReason::UnsupportedVersion => {
            "Unsupported workflow definition document version".to_string()
        }
        DocumentReason::IncompatibleLine => {
            "Incompatible workflow definition product major/minor line".to_string()
        }
        DocumentReason::DefinitionNotObject => "Definition must be an object".to_string(),
        DocumentReason::NonFiniteValue => "Definition must contain finite JSON values".to_string(),
        DocumentReason::InvalidName => NAME_RULE.to_string(),
        DocumentReason::NameMismatch => "Definition name does not match source name".to_string(),
        DocumentReason::Malformed(path) => format!("Malformed {path}"),
        DocumentReason::MalformedFallback => "Malformed agent fallback".to_string(),
        DocumentReason::MalformedObserveRepos => {
            "Malformed observe_repos: expected at most seven absolute paths".to_string()
        }
        DocumentReason::MalformedNode => "Malformed definition node".to_string(),
        DocumentReason::MalformedConnection => "Malformed definition connection".to_string(),
        DocumentReason::MalformedReference => "Malformed workflow reference identity".to_string(),
        DocumentReason::Os(text) => text.clone(),
    }
}
