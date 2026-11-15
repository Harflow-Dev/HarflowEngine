//! Closed vocabularies of a workflow definition.

/// The access levels of a node, ordered from least to most permissive.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Freedom {
    ReadOnly,
    WriteInRepo,
    Publish,
    Unrestricted,
}

impl Freedom {
    pub const ALL: [Freedom; 4] = [
        Freedom::ReadOnly,
        Freedom::WriteInRepo,
        Freedom::Publish,
        Freedom::Unrestricted,
    ];

    pub fn parse(text: &str) -> Option<Freedom> {
        Freedom::ALL.into_iter().find(|item| item.as_str() == text)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Freedom::ReadOnly => "read_only",
            Freedom::WriteInRepo => "write_in_repo",
            Freedom::Publish => "publish",
            Freedom::Unrestricted => "unrestricted",
        }
    }
}

/// The role an agent node plays.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Role {
    Planning,
    Review,
    Implementation,
    Task,
}

impl Role {
    pub fn parse(text: &str) -> Option<Role> {
        [
            Role::Planning,
            Role::Review,
            Role::Implementation,
            Role::Task,
        ]
        .into_iter()
        .find(|item| item.as_str() == text)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Planning => "planning",
            Role::Review => "review",
            Role::Implementation => "implementation",
            Role::Task => "task",
        }
    }

    /// The freedom a node of this role gets when it names none.
    pub fn default_freedom(self) -> Freedom {
        match self {
            Role::Planning | Role::Review => Freedom::ReadOnly,
            Role::Implementation => Freedom::WriteInRepo,
            Role::Task => Freedom::Publish,
        }
    }
}

/// The node types of a graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum NodeType {
    Start,
    Agent,
    Join,
    End,
    ParallelStart,
    ParallelEnd,
    Workflow,
}

impl NodeType {
    pub fn parse(text: &str) -> Option<NodeType> {
        [
            NodeType::Start,
            NodeType::Agent,
            NodeType::Join,
            NodeType::End,
            NodeType::ParallelStart,
            NodeType::ParallelEnd,
            NodeType::Workflow,
        ]
        .into_iter()
        .find(|item| item.as_str() == text)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            NodeType::Start => "start",
            NodeType::Agent => "agent",
            NodeType::Join => "join",
            NodeType::End => "end",
            NodeType::ParallelStart => "parallel_start",
            NodeType::ParallelEnd => "parallel_end",
            NodeType::Workflow => "workflow",
        }
    }

    /// Executable nodes dispatch work: an agent, or a whole referenced workflow.
    pub fn is_executable(self) -> bool {
        self == NodeType::Agent || self == NodeType::Workflow
    }
}

/// The reasoning efforts a candidate may name.
pub const EFFORTS: [&str; 4] = ["low", "medium", "high", "xhigh"];

/// The deepest nesting of referenced workflows.
pub const MAX_NESTING_DEPTH: usize = 4;

/// Bounds Python lacks: connected plus on-demand nodes, and connections.
pub const MAX_NODES: usize = 1000;
pub const MAX_CONNECTIONS: usize = 4000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freedom_order() {
        assert!(Freedom::ReadOnly < Freedom::WriteInRepo);
        assert!(Freedom::WriteInRepo < Freedom::Publish);
        assert!(Freedom::Publish < Freedom::Unrestricted);
        assert_eq!(Freedom::parse("publish"), Some(Freedom::Publish));
        assert_eq!(Freedom::parse("root"), None);
        assert_eq!(Role::Review.default_freedom(), Freedom::ReadOnly);
        assert_eq!(NodeType::parse("parallel_end"), Some(NodeType::ParallelEnd));
    }
}
