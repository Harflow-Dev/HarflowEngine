//! Refusals as values: a rule and the details its message interpolates. The message text
//! is rendered by the interface layer.
use super::json::Json;
use super::rule_graph::GraphRule;
use super::rule_shape::{CandidateRule, DefinitionRule, GitRule, NodeRule, OnDemandRule};

/// Which rule refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Rule {
    Dependency(DependencyRule),
    Definition(DefinitionRule),
    Node(NodeRule),
    OnDemand(OnDemandRule),
    Candidate(CandidateRule),
    Graph(GraphRule),
    Git(GitRule),
}

impl From<DefinitionRule> for Rule {
    fn from(rule: DefinitionRule) -> Self {
        Rule::Definition(rule)
    }
}

impl From<NodeRule> for Rule {
    fn from(rule: NodeRule) -> Self {
        Rule::Node(rule)
    }
}

impl From<OnDemandRule> for Rule {
    fn from(rule: OnDemandRule) -> Self {
        Rule::OnDemand(rule)
    }
}

impl From<CandidateRule> for Rule {
    fn from(rule: CandidateRule) -> Self {
        Rule::Candidate(rule)
    }
}

impl From<GraphRule> for Rule {
    fn from(rule: GraphRule) -> Self {
        Rule::Graph(rule)
    }
}

impl From<GitRule> for Rule {
    fn from(rule: GitRule) -> Self {
        Rule::Git(rule)
    }
}

/// A validation refusal: the rule and its interpolated values, in the order the rule lists.
#[derive(Clone, Debug, PartialEq)]
pub struct Refusal {
    pub rule: Rule,
    pub detail: Vec<Json>,
}

impl Refusal {
    pub fn new(rule: impl Into<Rule>) -> Self {
        Self {
            rule: rule.into(),
            detail: Vec::new(),
        }
    }

    /// This refusal with one more interpolated value.
    pub fn with(mut self, value: Json) -> Self {
        self.detail.push(value);
        self
    }

    /// This refusal with one more interpolated text.
    pub fn with_text(self, value: &str) -> Self {
        self.with(Json::text(value))
    }
}

/// Why a stored definition source cannot provide a qualified definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocumentReason {
    /// Missing or foreign product or format.
    ForeignFormat,
    UnsupportedVersion,
    IncompatibleLine,
    DefinitionNotObject,
    /// Containers nest deeper than `MAX_JSON_DEPTH`.
    TooDeep,
    NonFiniteValue,
    /// A name that is not a valid workflow name: the name text.
    InvalidName,
    NameMismatch,
    /// `Malformed <path>`: the dotted path such as `node.network`.
    Malformed(String),
    MalformedFallback,
    MalformedObserveRepos,
    MalformedNode,
    MalformedConnection,
    MalformedReference,
    /// An operating-system failure reading or listing the source.
    Os(String),
}

/// How a refused source is classified.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentCode {
    Incompatible,
    Unavailable,
}

/// A stored definition that cannot be used: the source name, a code and the reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentRefusal {
    pub name: String,
    pub code: DocumentCode,
    pub reason: DocumentReason,
}

/// One missing reference found while resolving a dependency tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissingReference {
    pub segment: ReferenceSegment,
    pub target_workflow_id: String,
    pub dependency_path: Vec<ReferenceSegment>,
}

/// One step of a path through nested workflow references.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceSegment {
    pub workflow_id: String,
    pub workflow_name: String,
    pub node_id: String,
    pub node_title: String,
}

/// A dependency tree that cannot be resolved: missing, cyclic or too deep.
#[derive(Clone, Debug, PartialEq)]
pub struct DependencyRefusal {
    pub refusal: Refusal,
    pub missing_references: Vec<MissingReference>,
    pub truncated: bool,
}

/// Dependency errors; details carry ids, node ids or the ordered name path.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum DependencyRule {
    Duplicate,
    Nesting,
    MissingIdentity,
    Cycle,
    Unknown,
    NotInTree,
}

impl From<DependencyRule> for Rule {
    fn from(rule: DependencyRule) -> Self {
        Self::Dependency(rule)
    }
}
