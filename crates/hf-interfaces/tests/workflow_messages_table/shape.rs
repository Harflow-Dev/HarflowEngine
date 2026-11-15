//! Definition, connection, node and on-demand rules.
use hf_domain::workflow::refusal::Rule::{Definition as D, Node as N, OnDemand as O};
use hf_domain::workflow::rule_shape::DefinitionRule as DR;
use hf_domain::workflow::rule_shape::NodeRule as NR;
use hf_domain::workflow::rule_shape::OnDemandRule as OR;

use super::{Entry, IDENTIFIER_RULE, NAME_RULE};

pub const DEFINITION: &[Entry] = &[
    (
        D(DR::NotObject),
        "[]",
        "Workflow definition must be an object",
    ),
    (
        D(DR::StartPromptNotText),
        "[]",
        "Start prompt must be a string",
    ),
    (D(DR::InvalidWorkflowName), "[]", NAME_RULE),
    (
        D(DR::InvalidContextDelivery),
        "[]",
        "context_delivery must be legacy or optimized_v1",
    ),
    (D(DR::InvalidRoutingMode), "[]", "Invalid routing_mode"),
    (D(DR::DescriptionNotText), "[]", "Description must be text"),
    (
        D(DR::OrchestratorNotObject),
        "[]",
        "Orchestrator must be an agent object",
    ),
    (
        D(DR::OrchestratorInstructionsNotText),
        "[]",
        "Orchestrator instructions must be text",
    ),
    (
        D(DR::IntegerOutOfRange),
        r#"["max_parallel", 64]"#,
        "max_parallel must be an integer between 1 and 64",
    ),
    (
        D(DR::NodesAndConnectionsRequired),
        "[]",
        "Workflow requires nodes and connections arrays",
    ),
    (
        D(DR::TooManyNodes),
        "[]",
        "Workflow supports at most 1000 nodes, on-demand nodes included",
    ),
    (
        D(DR::TooManyConnections),
        "[]",
        "Workflow supports at most 4000 connections",
    ),
    (
        D(DR::StartEndRequired),
        "[]",
        "Workflow needs exactly one Start and at least one End",
    ),
    (
        D(DR::ConnectionNotObject),
        "[]",
        "Every connection must be an object",
    ),
    (
        D(DR::DuplicateConnection),
        r#"["c1"]"#,
        "Duplicate connection c1",
    ),
    (
        D(DR::ConnectionUnknownNode),
        r#"["c1"]"#,
        "Connection c1 references an unknown node",
    ),
    (
        D(DR::EndStartConnection),
        "[]",
        "End cannot have outputs and Start cannot have inputs",
    ),
    (
        D(DR::MaxRetriesInvalid),
        r#"["c1"]"#,
        "max_retries must be a nonnegative integer: c1",
    ),
    (
        D(DR::ConnectionFlagsInvalid),
        r#"["c1"]"#,
        "Invalid condition or connection flags: c1",
    ),
    (
        D(DR::NoOutgoingConnection),
        r#"["n"]"#,
        "Node n has no outgoing connection",
    ),
    (
        D(DR::MultipleDefaults),
        r#"["n"]"#,
        "Node n has multiple default connections",
    ),
];

pub const NODE: &[Entry] = &[
    (N(NR::NodeNotObject), "[]", "Every node must be an object"),
    (N(NR::InvalidIdentifier), "[]", IDENTIFIER_RULE),
    (N(NR::DuplicateNode), r#"["n"]"#, "Duplicate node n"),
    (N(NR::InvalidNodeType), r#"["n"]"#, "Invalid node type: n"),
    (
        N(NR::ParallelNeedsExplicitRouting),
        "[]",
        "Parallel boundaries require explicit routing_mode",
    ),
    (
        N(NR::InvalidBranchSelection),
        "[]",
        "Parallel branch_selection must be all or orchestrator",
    ),
    (
        N(NR::SelectionGuidanceNotText),
        "[]",
        "Parallel selection_guidance must be a string",
    ),
    (
        N(NR::AlwaysRouteNotBool),
        "[]",
        "Node always_route must be a boolean",
    ),
    (
        N(NR::InvalidPurpose),
        r#"["n"]"#,
        "Node purpose must be a nonempty single line of at most 500 characters: n",
    ),
    (
        N(NR::InvalidTitleOrPosition),
        r#"["n"]"#,
        "Invalid title or canvas position (x and y must be finite and nonnegative): n",
    ),
    (
        N(NR::InvalidBranchMode),
        r#"["n"]"#,
        "Invalid branching mode: n",
    ),
    (
        N(NR::WorkflowNeedsExplicitRouting),
        r#"["n"]"#,
        "Run workflow nodes require explicit routing_mode: n",
    ),
    (
        N(NR::WorkflowRejectsField),
        r#"["agent", "n"]"#,
        "Run workflow nodes do not accept agent: n",
    ),
    (
        N(NR::WorkflowRefRequired),
        r#"["n"]"#,
        "Run workflow node requires workflow_ref.workflow_id: n",
    ),
    (
        N(NR::WorkflowNameNotText),
        r#"["n"]"#,
        "Run workflow workflow_name must be text: n",
    ),
    (
        N(NR::InvalidOrchestratorMode),
        r#"["n"]"#,
        "Run workflow orchestrator_mode must be child or current: n",
    ),
    (
        N(NR::InvalidChildSessionPolicy),
        r#"["n"]"#,
        "Invalid child_session_policy: n",
    ),
    (
        N(NR::WorkflowInstructionsNotText),
        r#"["n"]"#,
        "Run workflow instructions must be text: n",
    ),
    (
        N(NR::WorkflowOptionalNotBool),
        r#"["n"]"#,
        "Run workflow optional must be a boolean: n",
    ),
    (
        N(NR::WorkflowTimeoutInvalid),
        r#"["n"]"#,
        "timeout_seconds must be a nonnegative integer: n",
    ),
    (
        N(NR::AgentOptionalNotBool),
        "[]",
        "Agent optional must be a boolean",
    ),
    (N(NR::InvalidRole), r#"["n"]"#, "Invalid agent role: n"),
    (
        N(NR::PlanningFlagNotBool),
        r#"["require_tasks"]"#,
        "Planning require_tasks must be a boolean",
    ),
    (
        N(NR::InvalidInstructionsOrNetwork),
        r#"["n"]"#,
        "Invalid instructions or network: n",
    ),
    (
        N(NR::InvalidFreedom),
        "[]",
        "Workflow nodes support read_only, write_in_repo, publish or unrestricted",
    ),
    (
        N(NR::ImplementationReadOnly),
        "[]",
        "Implementation nodes cannot use read_only",
    ),
    (
        N(NR::InvalidExecutionMode),
        r#"["n"]"#,
        "Invalid execution mode: n",
    ),
    (
        N(NR::InvalidSessionMode),
        r#"["n"]"#,
        "Invalid session mode: n",
    ),
    (
        N(NR::TimeoutInvalid),
        "[]",
        "timeout_seconds must be a nonnegative integer",
    ),
    (
        N(NR::GroupReportNeedsReadOnly),
        r#"["n"]"#,
        "Node n: group_report_output requires read_only",
    ),
    (
        N(NR::AllowedCommandsRefused),
        r#"["n", "claude", "why"]"#,
        "Node n claude candidate: why",
    ),
];

pub const ON_DEMAND: &[Entry] = &[
    (
        O(OR::OnDemandNotArray),
        "[]",
        "on_demand_nodes must be an array",
    ),
    (
        O(OR::OnDemandKeys),
        "[]",
        "On-demand nodes accept only id, title, instructions, call_condition, agent, freedom, network, session_mode, execution_mode and resources",
    ),
    (
        O(OR::OnDemandInstructions),
        r#"["n"]"#,
        "On-demand instructions must be nonempty text: n",
    ),
    (
        O(OR::OnDemandTitleOrCondition),
        r#"["n"]"#,
        "On-demand title and call_condition must be text: n",
    ),
    (
        O(OR::OnDemandFreedomOrNetwork),
        r#"["n"]"#,
        "Invalid on-demand freedom or network: n",
    ),
    (
        O(OR::OnDemandSessionMode),
        r#"["n"]"#,
        "Invalid on-demand session_mode: n",
    ),
    (
        O(OR::OnDemandExecutionMode),
        r#"["n"]"#,
        "Invalid on-demand execution_mode: n",
    ),
    (
        O(OR::ResourcesOnlyAgents),
        r#"["n"]"#,
        "Only agent nodes accept resources: n",
    ),
    (
        O(OR::ResourceNamesNotList),
        "[]",
        "resource names must be a list of names",
    ),
    (
        O(OR::ResourceNamesSingleString),
        "[]",
        "resource names must be a list of names, not a single string",
    ),
    (
        O(OR::ResourceCount),
        "[17]",
        "request 1 to 16 resource names, got 17",
    ),
    (
        O(OR::ResourceNameInvalid),
        r#"["Bad"]"#,
        "invalid resource name 'Bad': use lowercase letters, digits and . _ : - (1 to 64 characters, starting with a letter or digit)",
    ),
    (
        O(OR::ResourceDuplicate),
        "[]",
        "duplicate resource names in one request",
    ),
    (
        O(OR::ResourcePreferSubagent),
        r#"["n"]"#,
        "prefer_subagent nodes cannot declare resources: n",
    ),
    (
        O(OR::ResourceSubset),
        r#"["n"]"#,
        "Node resources must be a subset of run resources: n",
    ),
];
