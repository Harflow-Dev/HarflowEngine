//! The refusals of definition, node, candidate and resource shape. The detail values a
//! rule carries are listed beside it, in order; `id` is a node or connection id.

/// Refusals of the definition as a whole and of its connections.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum DefinitionRule {
    /// The definition nests containers deeper than `MAX_JSON_DEPTH`.
    TooDeep,
    /// The definition is not an object.
    NotObject,
    StartPromptNotText,
    InvalidWorkflowName,
    InvalidContextDelivery,
    InvalidRoutingMode,
    DescriptionNotText,
    OrchestratorNotObject,
    OrchestratorInstructionsNotText,
    /// `field`, `maximum`.
    IntegerOutOfRange,
    NodesAndConnectionsRequired,
    TooManyNodes,
    TooManyConnections,
    StartEndRequired,
    ConnectionNotObject,
    /// `id`.
    DuplicateConnection,
    /// `id`.
    ConnectionUnknownNode,
    EndStartConnection,
    /// `id`.
    MaxRetriesInvalid,
    /// `id`.
    ConnectionFlagsInvalid,
    /// `id`.
    NoOutgoingConnection,
    /// `id`.
    MultipleDefaults,
}

/// Refusals of connected nodes: shared fields, Run workflow nodes and agent nodes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum NodeRule {
    NodeNotObject,
    InvalidIdentifier,
    /// `id`.
    DuplicateNode,
    /// `id`.
    InvalidNodeType,
    ParallelNeedsExplicitRouting,
    InvalidBranchSelection,
    SelectionGuidanceNotText,
    AlwaysRouteNotBool,
    /// `id`.
    InvalidPurpose,
    /// `id`.
    InvalidTitleOrPosition,
    /// `id`.
    InvalidBranchMode,
    /// `id`.
    WorkflowNeedsExplicitRouting,
    /// `field`, `id`.
    WorkflowRejectsField,
    /// `id`.
    WorkflowRefRequired,
    /// `id`.
    WorkflowNameNotText,
    /// `id`.
    InvalidOrchestratorMode,
    /// `id`.
    InvalidChildSessionPolicy,
    /// `id`.
    WorkflowInstructionsNotText,
    /// `id`.
    WorkflowOptionalNotBool,
    /// `id`.
    WorkflowTimeoutInvalid,
    AgentOptionalNotBool,
    /// `id`.
    InvalidRole,
    /// `field`.
    PlanningFlagNotBool,
    /// `id`.
    InvalidInstructionsOrNetwork,
    InvalidFreedom,
    ImplementationReadOnly,
    /// `id`.
    InvalidExecutionMode,
    /// `id`.
    InvalidSessionMode,
    TimeoutInvalid,
    /// `label`.
    GroupReportNeedsReadOnly,
    /// `label`, `backend`, `reason`.
    AllowedCommandsRefused,
}

/// Refusals of on-demand nodes and of resource declarations.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum OnDemandRule {
    OnDemandNotArray,
    OnDemandKeys,
    /// `id`.
    OnDemandInstructions,
    /// `id`.
    OnDemandTitleOrCondition,
    /// `id`.
    OnDemandFreedomOrNetwork,
    /// `id`.
    OnDemandSessionMode,
    /// `id`.
    OnDemandExecutionMode,
    /// `id`.
    ResourcesOnlyAgents,
    ResourceNamesNotList,
    ResourceNamesSingleString,
    /// `count`.
    ResourceCount,
    /// `name`.
    ResourceNameInvalid,
    ResourceDuplicate,
    /// `id`.
    ResourcePreferSubagent,
    /// `id`.
    ResourceSubset,
}

/// Refusals of an agent candidate and its fallbacks.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CandidateRule {
    CandidateNotObject,
    /// The sorted unknown keys.
    UnknownAgentSettings,
    InvalidConversationProfile,
    FallbacksNotList,
    /// The backend value, the sorted known names.
    UnknownBackend,
    CallerApprovalsNotBool,
    CallerWriteApprovalsNotBool,
    CallerWriteNeedsApprovals,
    GroupReportNotBool,
    GroupReportNotAllowedHere,
    GroupReportWithApprovals,
    GitHttpsRewriteNotBool,
    ObserveReposNotAllowedHere,
    ObserveReposBounds,
    ObserveReposAbsolute,
    /// `reason` from the resolver.
    ObserveReposResolve,
    AllowedCommandsNotAllowedHere,
    /// `reason` from the backend catalog.
    AllowedCommandsBounds,
    CallerApprovalsUnqualified,
    ModelNotText,
    BackgroundIdleInvalid,
    InvalidReasoningEffort,
    /// `backend`, `model`.
    NoModelSelection,
    /// `backend`, `max_turns`.
    NoTurnCap,
    /// `backend`, `effort`.
    NoEffortControl,
    /// `backend`, `effort`, the accepted levels.
    EffortNotAccepted,
    FallbacksNotFlat,
}

/// The refusals of the Git preparation a node asks for. Each carries the node id as its
/// first detail, or `null` for none.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum GitRule {
    /// The label.
    LiteralName,
    BaseForm,
    FetchForm,
    FetchEmpty,
    FetchRequiredNotBool,
    FetchRequiredNeedsFetch,
    BranchBaseTogether,
    BranchContinuation,
    BranchReadOnly,
    BranchWorktree,
}
