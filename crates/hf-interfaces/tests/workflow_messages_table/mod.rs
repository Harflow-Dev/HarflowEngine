//! Every validation rule with the detail values it carries and the text it renders to.
use hf_domain::workflow::refusal::Rule;

mod candidate;
mod graph;
mod shape;

/// A rule, its detail values as JSON text, and the exact message.
pub type Entry = (Rule, &'static str, &'static str);

pub const NAME_RULE: &str = "Workflow names must contain 1–100 letters, digits, spaces, dots, dashes or underscores, start with a letter or digit, and have no trailing spaces";

pub const IDENTIFIER_RULE: &str =
    "Identifiers must contain 1–100 letters, digits, dots, dashes or underscores";

/// The number of rules of each kind: definition, node, on-demand, candidate, graph, git.
pub const RULE_COUNTS: [usize; 6] = [21, 31, 15, 27, 22, 10];

pub fn tables() -> [&'static [Entry]; 6] {
    [
        shape::DEFINITION,
        shape::NODE,
        shape::ON_DEMAND,
        candidate::CANDIDATE,
        graph::GRAPH,
        graph::GIT,
    ]
}
