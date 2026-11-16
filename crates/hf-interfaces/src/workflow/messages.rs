//! The text of validation refusals, byte for byte as Python wrote it, including the en
//! dash of the name and identifier messages.
use hf_application::workflow::validate::WorkflowFailure;
use hf_domain::workflow::json::Json;
use hf_domain::workflow::refusal::{DependencyRule, Refusal, Rule};
use hf_domain::workflow::rule_shape::{DefinitionRule, NodeRule};

use super::messages_candidate::{candidate_message, on_demand_message};
use super::messages_graph::{git_message, graph_message, reason_message};
use super::repr::repr_of;

/// The interpolated values of a refusal.
pub(super) struct Args<'a>(pub &'a [Json]);

impl Args<'_> {
    /// `str(value)`: a string as itself, an integer as its digits.
    pub fn text(&self, index: usize) -> String {
        match self.0.get(index) {
            Some(Json::Str(text)) => text.clone(),
            Some(Json::Int(number)) => number.text(),
            Some(other) => repr_of(other),
            None => String::new(),
        }
    }

    /// `repr(value)`.
    pub fn repr(&self, index: usize) -> String {
        self.0.get(index).map(repr_of).unwrap_or_default()
    }

    /// The strings of a list value, joined by `, `.
    pub fn joined(&self, index: usize) -> String {
        match self.0.get(index) {
            Some(Json::Array(items)) => items
                .iter()
                .map(|item| item.as_str().unwrap_or(""))
                .collect::<Vec<_>>()
                .join(", "),
            _ => String::new(),
        }
    }
}

pub(super) const NAME_RULE: &str = "Workflow names must contain 1–100 letters, digits, spaces, dots, dashes or underscores, start with a letter or digit, and have no trailing spaces";

/// The message of a validation refusal.
pub fn message(refusal: &Refusal) -> String {
    let args = Args(&refusal.detail);
    match refusal.rule {
        Rule::Dependency(rule) => dependency_message(rule, &args),
        Rule::Definition(rule) => definition_message(rule, &args),
        Rule::Node(rule) => node_message(rule, &args),
        Rule::OnDemand(rule) => on_demand_message(rule, &args),
        Rule::Candidate(rule) => candidate_message(rule, &args),
        Rule::Graph(rule) => graph_message(rule, &args),
        Rule::Git(rule) => git_message(rule, &args),
    }
}

/// The message of a refusal or of a stored-document qualification failure.
pub fn failure_message(failure: &WorkflowFailure) -> String {
    match failure {
        WorkflowFailure::Refused(refusal) => message(refusal),
        WorkflowFailure::Qualification(reason) => reason_message(reason),
    }
}

fn definition_message(rule: DefinitionRule, a: &Args<'_>) -> String {
    match rule {
        DefinitionRule::TooDeep => "Workflow definition nests deeper than 64 levels".to_string(),
        DefinitionRule::NotObject => "Workflow definition must be an object".to_string(),
        DefinitionRule::StartPromptNotText => "Start prompt must be a string".to_string(),
        DefinitionRule::InvalidWorkflowName => NAME_RULE.to_string(),
        DefinitionRule::InvalidContextDelivery => {
            "context_delivery must be legacy or optimized_v1".to_string()
        }
        DefinitionRule::InvalidRoutingMode => "Invalid routing_mode".to_string(),
        DefinitionRule::DescriptionNotText => "Description must be text".to_string(),
        DefinitionRule::OrchestratorNotObject => "Orchestrator must be an agent object".to_string(),
        DefinitionRule::OrchestratorInstructionsNotText => {
            "Orchestrator instructions must be text".to_string()
        }
        DefinitionRule::IntegerOutOfRange => {
            format!(
                "{} must be an integer between 1 and {}",
                a.text(0),
                a.text(1)
            )
        }
        DefinitionRule::NodesAndConnectionsRequired => {
            "Workflow requires nodes and connections arrays".to_string()
        }
        DefinitionRule::TooManyNodes => {
            "Workflow supports at most 1000 nodes, on-demand nodes included".to_string()
        }
        DefinitionRule::TooManyConnections => {
            "Workflow supports at most 4000 connections".to_string()
        }
        DefinitionRule::StartEndRequired => {
            "Workflow needs exactly one Start and at least one End".to_string()
        }
        DefinitionRule::ConnectionNotObject => "Every connection must be an object".to_string(),
        DefinitionRule::DuplicateConnection => format!("Duplicate connection {}", a.text(0)),
        DefinitionRule::ConnectionUnknownNode => {
            format!("Connection {} references an unknown node", a.text(0))
        }
        DefinitionRule::EndStartConnection => {
            "End cannot have outputs and Start cannot have inputs".to_string()
        }
        DefinitionRule::MaxRetriesInvalid => {
            format!("max_retries must be a nonnegative integer: {}", a.text(0))
        }
        DefinitionRule::ConnectionFlagsInvalid => {
            format!("Invalid condition or connection flags: {}", a.text(0))
        }
        DefinitionRule::NoOutgoingConnection => {
            format!("Node {} has no outgoing connection", a.text(0))
        }
        DefinitionRule::MultipleDefaults => {
            format!("Node {} has multiple default connections", a.text(0))
        }
    }
}

fn node_message(rule: NodeRule, a: &Args<'_>) -> String {
    let id = a.text(0);
    match rule {
        NodeRule::NodeNotObject => "Every node must be an object".to_string(),
        NodeRule::InvalidIdentifier => {
            "Identifiers must contain 1–100 letters, digits, dots, dashes or underscores"
                .to_string()
        }
        NodeRule::DuplicateNode => format!("Duplicate node {id}"),
        NodeRule::InvalidNodeType => format!("Invalid node type: {id}"),
        NodeRule::ParallelNeedsExplicitRouting => {
            "Parallel boundaries require explicit routing_mode".to_string()
        }
        NodeRule::InvalidBranchSelection => {
            "Parallel branch_selection must be all or orchestrator".to_string()
        }
        NodeRule::SelectionGuidanceNotText => {
            "Parallel selection_guidance must be a string".to_string()
        }
        NodeRule::AlwaysRouteNotBool => "Node always_route must be a boolean".to_string(),
        NodeRule::InvalidPurpose => {
            format!("Node purpose must be a nonempty single line of at most 500 characters: {id}")
        }
        NodeRule::InvalidTitleOrPosition => format!(
            "Invalid title or canvas position (x and y must be finite and nonnegative): {id}"
        ),
        NodeRule::InvalidBranchMode => format!("Invalid branching mode: {id}"),
        NodeRule::WorkflowNeedsExplicitRouting => {
            format!("Run workflow nodes require explicit routing_mode: {id}")
        }
        NodeRule::WorkflowRejectsField => {
            format!("Run workflow nodes do not accept {id}: {}", a.text(1))
        }
        NodeRule::WorkflowRefRequired => {
            format!("Run workflow node requires workflow_ref.workflow_id: {id}")
        }
        NodeRule::WorkflowNameNotText => format!("Run workflow workflow_name must be text: {id}"),
        NodeRule::InvalidOrchestratorMode => {
            format!("Run workflow orchestrator_mode must be child or current: {id}")
        }
        NodeRule::InvalidChildSessionPolicy => format!("Invalid child_session_policy: {id}"),
        NodeRule::WorkflowInstructionsNotText => {
            format!("Run workflow instructions must be text: {id}")
        }
        NodeRule::WorkflowOptionalNotBool => {
            format!("Run workflow optional must be a boolean: {id}")
        }
        NodeRule::WorkflowTimeoutInvalid => {
            format!("timeout_seconds must be a nonnegative integer: {id}")
        }
        NodeRule::AgentOptionalNotBool => "Agent optional must be a boolean".to_string(),
        NodeRule::InvalidRole => format!("Invalid agent role: {id}"),
        NodeRule::PlanningFlagNotBool => format!("Planning {id} must be a boolean"),
        NodeRule::InvalidInstructionsOrNetwork => format!("Invalid instructions or network: {id}"),
        NodeRule::InvalidFreedom => {
            "Workflow nodes support read_only, write_in_repo, publish or unrestricted".to_string()
        }
        NodeRule::ImplementationReadOnly => "Implementation nodes cannot use read_only".to_string(),
        NodeRule::InvalidExecutionMode => format!("Invalid execution mode: {id}"),
        NodeRule::InvalidSessionMode => format!("Invalid session mode: {id}"),
        NodeRule::TimeoutInvalid => "timeout_seconds must be a nonnegative integer".to_string(),
        NodeRule::GroupReportNeedsReadOnly => {
            format!("Node {id}: group_report_output requires read_only")
        }
        NodeRule::AllowedCommandsRefused => {
            format!("Node {id} {} candidate: {}", a.text(1), a.text(2))
        }
    }
}

fn dependency_message(rule: DependencyRule, args: &Args<'_>) -> String {
    let first = args.text(0);
    match rule {
        DependencyRule::Duplicate => {
            format!("Duplicate workflow id {first} names different definitions")
        }
        DependencyRule::Nesting => format!("Workflow nesting exceeds 4 levels: {first}"),
        DependencyRule::MissingIdentity => {
            format!("Run workflow node {first} has no workflow_id reference")
        }
        DependencyRule::Cycle => format!("Workflow reference cycle: {first}"),
        DependencyRule::Unknown => format!(
            "Unknown workflow reference {first} at node {}",
            args.text(1)
        ),
        DependencyRule::NotInTree => {
            format!("Workflow {first} is not part of this dependency tree")
        }
    }
}
