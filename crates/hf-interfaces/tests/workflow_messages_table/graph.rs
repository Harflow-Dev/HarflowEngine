//! Graph and Git rules.
use hf_domain::workflow::refusal::Rule::{Git as T, Graph as G};
use hf_domain::workflow::rule_graph::GraphRule as GR;
use hf_domain::workflow::rule_shape::GitRule as TR;

use super::Entry;

pub const GRAPH: &[Entry] = &[
    (
        G(GR::UnreachableFromStart),
        "[]",
        "All nodes must be reachable from Start",
    ),
    (
        G(GR::AmbiguousLoop),
        "[]",
        "Ambiguous loop: cycles must return to a step that dominates the retrying step",
    ),
    (
        G(GR::NotForwardReachable),
        "[]",
        "All nodes must be forward-reachable from Start",
    ),
    (
        G(GR::NoForwardPathToEnd),
        r#"[["a", "b"]]"#,
        "Every node must have a forward path to End: a, b",
    ),
    (
        G(GR::ParallelUseBoundaries),
        "[]",
        "Use Parallel start and Parallel end instead of Join",
    ),
    (
        G(GR::ParallelDuplicateBoundary),
        r#"["parallel_start", "g"]"#,
        "Duplicate parallel_start for parallel group g",
    ),
    (
        G(GR::ParallelGroupIncomplete),
        r#"["g"]"#,
        "Parallel group g requires matching start and end",
    ),
    (
        G(GR::ParallelStartBranches),
        r#"["p"]"#,
        "Parallel start p requires at least two forward branches and no retry outputs",
    ),
    (
        G(GR::ParallelBranchMissesEnd),
        r#"["p", "q"]"#,
        "Every branch from Parallel start p must reach matching Parallel end q",
    ),
    (
        G(GR::ParallelEmptyBranch),
        r#"["p"]"#,
        "Parallel start p cannot have an empty branch",
    ),
    (
        G(GR::ParallelOverlap),
        r#"["g"]"#,
        "Parallel group g has crossing or overlapping sibling branches",
    ),
    (
        G(GR::ParallelReachesEnd),
        r#"["g"]"#,
        "Parallel group g reaches workflow End before its matching end",
    ),
    (
        G(GR::ParallelImproperNesting),
        r#"["g"]"#,
        "Parallel group g contains an improperly nested boundary",
    ),
    (
        G(GR::ParallelExternalEntry),
        r#"["g", "c1"]"#,
        "External entry into parallel branch g: c1",
    ),
    (
        G(GR::ParallelEscape),
        r#"["g", "c1"]"#,
        "Connection escapes parallel branch g: c1",
    ),
    (
        G(GR::ParallelRetryEntry),
        r#"["c1"]"#,
        "Retry cannot enter a closed parallel branch: c1",
    ),
    (
        G(GR::ParallelEndExternalEntry),
        r#"["q"]"#,
        "External entry into Parallel end q",
    ),
    (
        G(GR::LegacyNoConvergence),
        r#"["p"]"#,
        "Parallel branches from p must converge at one common node before ending",
    ),
    (
        G(GR::LegacyCrossBranch),
        r#"["p"]"#,
        "Cross-branch connection at split p: its legacy Join follows an earlier convergence; connect branches directly to their first shared node",
    ),
    (
        G(GR::JoinOutsideSplit),
        r#"["j"]"#,
        "Join j is outside its matching split",
    ),
    (
        G(GR::SelectionJoinAmbiguous),
        "[]",
        "Selected parallel paths must converge at one common node",
    ),
    (
        G(GR::OptionalUnsafe),
        r#"[["a", "b"]]"#,
        "Optional nodes need a safe parallel branch with a required sibling and no required successor before convergence: a, b",
    ),
];

/// The git rules carry the node prefix as their first detail; these use `n1`.
pub const GIT: &[Entry] = &[
    (
        T(TR::LiteralName),
        r#"["n1", "branch"]"#,
        "n1: branch must be a literal name of letters, digits, '_', '-' and '/', starting with a letter or digit, with no empty, '.' or '..' component, and no options, refspecs or revision expressions",
    ),
    (
        T(TR::BaseForm),
        r#"["n1"]"#,
        "n1: base must be <remote>/<branch>, for example origin/develop; the first component is the remote and the rest is the branch",
    ),
    (
        T(TR::FetchForm),
        r#"["n1"]"#,
        "n1: fetch must be true or a list of remote names",
    ),
    (
        T(TR::FetchEmpty),
        r#"["n1"]"#,
        "n1: fetch must name at least one remote, or be true for the default remote",
    ),
    (
        T(TR::FetchRequiredNotBool),
        r#"["n1"]"#,
        "n1: fetch_required must be a boolean",
    ),
    (
        T(TR::FetchRequiredNeedsFetch),
        r#"["n1"]"#,
        "n1: fetch_required needs fetch: true or a list of remote names",
    ),
    (
        T(TR::BranchBaseTogether),
        r#"["n1"]"#,
        "n1: branch and base are set together; pass both, for example branch='feature/x' and base='origin/develop'",
    ),
    (
        T(TR::BranchContinuation),
        r#"["n1"]"#,
        "n1: continuations reuse the branch prepared at dispatch and do not accept branch or base",
    ),
    (
        T(TR::BranchReadOnly),
        r#"["n1"]"#,
        "n1: branch preparation is unavailable at read_only; use write_in_repo or above, or pass fetch without branch",
    ),
    (
        T(TR::BranchWorktree),
        r#"["n1"]"#,
        "n1: branch preparation and worktree create different checkouts; pass one of them",
    ),
];
