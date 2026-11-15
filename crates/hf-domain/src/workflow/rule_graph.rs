//! The refusals of graph structure. The detail values a rule carries are listed beside it,
//! in order; `id` is a node or connection id.

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum GraphRule {
    UnreachableFromStart,
    AmbiguousLoop,
    NotForwardReachable,
    /// The sorted ids of the nodes without a forward path to End.
    NoForwardPathToEnd,
    ParallelUseBoundaries,
    /// The node type, the group.
    ParallelDuplicateBoundary,
    /// The group.
    ParallelGroupIncomplete,
    /// The start id.
    ParallelStartBranches,
    /// The start id, the end id.
    ParallelBranchMissesEnd,
    /// The start id.
    ParallelEmptyBranch,
    /// The group.
    ParallelOverlap,
    /// The group.
    ParallelReachesEnd,
    /// The group.
    ParallelImproperNesting,
    /// The group, the connection id.
    ParallelExternalEntry,
    /// The group, the connection id.
    ParallelEscape,
    /// The connection id.
    ParallelRetryEntry,
    /// The end id.
    ParallelEndExternalEntry,
    /// The split id.
    LegacyNoConvergence,
    /// The split id.
    LegacyCrossBranch,
    /// The join id.
    JoinOutsideSplit,
    SelectionJoinAmbiguous,
    /// The sorted ids of the optional nodes.
    OptionalUnsafe,
}
