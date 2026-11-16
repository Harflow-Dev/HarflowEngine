//! The text of validation refusals: every rule renders, the en dash survives, and Python's
//! `repr` is reproduced for interpolated values.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_messages_table;

use std::collections::HashSet;

use hf_application::workflow::validate::WorkflowFailure;
use hf_domain::workflow::json::Json;
use hf_domain::workflow::refusal::{DocumentReason, Refusal, Rule};
use hf_domain::workflow::rule_shape::{DefinitionRule, GitRule, NodeRule};
use hf_interfaces::workflow::json_text::parse;
use hf_interfaces::workflow::messages::{failure_message, message};
use hf_interfaces::workflow::{reason_message, repr::repr_of};
use workflow_messages_table::{IDENTIFIER_RULE, NAME_RULE, RULE_COUNTS, tables};

fn read(text: &str) -> Json {
    parse(text).unwrap()
}

fn refusal(rule: Rule, detail: &str) -> Refusal {
    let Json::Array(values) = read(detail) else {
        panic!("detail {detail} is not a list");
    };
    Refusal {
        rule,
        detail: values,
    }
}

#[test]
fn every_rule_renders() {
    let mut seen: HashSet<Rule> = HashSet::new();
    for (table, count) in tables().iter().zip(RULE_COUNTS) {
        assert_eq!(table.len(), count, "a rule has no entry in the table");
        for (rule, detail, expected) in table.iter() {
            assert!(seen.insert(*rule), "{rule:?} is listed twice");
            let text = message(&refusal(*rule, detail));
            assert_eq!(&text, expected, "{rule:?}");
            assert!(!text.is_empty(), "{rule:?} renders empty");
        }
    }
    assert_eq!(seen.len(), RULE_COUNTS.iter().sum::<usize>());
}

#[test]
fn en_dash_preserved() {
    assert!(NAME_RULE.contains('\u{2013}'));
    assert!(!NAME_RULE.contains("1-100"));
    let name = message(&Refusal::new(DefinitionRule::InvalidWorkflowName));
    assert!(name.starts_with("Workflow names must contain 1\u{2013}100 letters"));
    let identifier = message(&Refusal::new(NodeRule::InvalidIdentifier));
    assert!(identifier.starts_with("Identifiers must contain 1\u{2013}100 letters"));
    assert_eq!(identifier, IDENTIFIER_RULE);
    assert_eq!(reason_message(&DocumentReason::InvalidName), NAME_RULE);
    let failure = WorkflowFailure::Refused(Refusal::new(DefinitionRule::InvalidWorkflowName));
    assert_eq!(
        failure_message(&failure)
            .as_bytes()
            .windows(3)
            .filter(|w| *w == [0xe2, 0x80, 0x93])
            .count(),
        1
    );
}

#[test]
fn repr_of_json_values() {
    let table = [
        ("null", "None"),
        ("true", "True"),
        ("false", "False"),
        ("12", "12"),
        ("-7", "-7"),
        (
            "123456789012345678901234567890",
            "123456789012345678901234567890",
        ),
        ("1.0", "1.0"),
        ("1e16", "1e+16"),
        ("-0.0", "-0.0"),
        ("NaN", "nan"),
        ("Infinity", "inf"),
        ("-Infinity", "-inf"),
        (r#""abc""#, "'abc'"),
        (r#""it's""#, r#""it's""#),
        (r#""say \"hi\"""#, r#"'say "hi"'"#),
        (r#""it's \"x\"""#, r#"'it\'s "x"'"#),
        (r#""a\nb\tc\\""#, r"'a\nb\tc\\'"),
        (r#""é""#, "'é'"),
        ("[]", "[]"),
        ("{}", "{}"),
        (r#"[1, "a", null, [true]]"#, "[1, 'a', None, [True]]"),
        (
            r#"{"a": 1, "b": [2.5], "c": {"d": null}}"#,
            "{'a': 1, 'b': [2.5], 'c': {'d': None}}",
        ),
    ];
    for (source, expected) in table {
        assert_eq!(repr_of(&read(source)), expected, "{source}");
    }
}

#[test]
fn interpolated_values_use_str_and_repr() {
    let text = |rule: Rule, detail: &str| message(&refusal(rule, detail));
    let duplicate: Rule = NodeRule::DuplicateNode.into();
    assert_eq!(text(duplicate, "[5]"), "Duplicate node 5");
    assert_eq!(text(duplicate, "[true]"), "Duplicate node True");
    assert_eq!(text(duplicate, "[null]"), "Duplicate node None");
    assert_eq!(text(duplicate, "[1.5]"), "Duplicate node 1.5");
    assert_eq!(text(duplicate, "[\"é\"]"), "Duplicate node é");
    let unknown: Rule = hf_domain::workflow::rule_shape::CandidateRule::UnknownBackend.into();
    assert_eq!(
        text(unknown, r#"[5, []]"#),
        "unknown backend 5; expected one of []"
    );
    assert_eq!(
        text(unknown, r#"[null, ["a"]]"#),
        "unknown backend None; expected one of ['a']"
    );
}

#[test]
fn missing_details_render_empty() {
    let bare = |rule: Rule| message(&Refusal::new(rule));
    assert_eq!(bare(NodeRule::DuplicateNode.into()), "Duplicate node ");
    assert_eq!(
        bare(DefinitionRule::IntegerOutOfRange.into()),
        " must be an integer between 1 and "
    );
    assert_eq!(
        bare(GitRule::BaseForm.into()),
        "base must be <remote>/<branch>, for example origin/develop; the first component is the remote and the rest is the branch"
    );
}

#[test]
fn git_prefix_only_when_text() {
    let none = message(&refusal(GitRule::FetchForm.into(), "[null]"));
    assert_eq!(none, "fetch must be true or a list of remote names");
    let named = message(&refusal(GitRule::FetchForm.into(), r#"["node-1"]"#));
    assert_eq!(
        named,
        "node-1: fetch must be true or a list of remote names"
    );
}

#[test]
fn document_reasons_render() {
    let table = [
        (
            DocumentReason::ForeignFormat,
            "Missing or foreign workflow definition product/format",
        ),
        (
            DocumentReason::UnsupportedVersion,
            "Unsupported workflow definition document version",
        ),
        (
            DocumentReason::IncompatibleLine,
            "Incompatible workflow definition product major/minor line",
        ),
        (
            DocumentReason::DefinitionNotObject,
            "Definition must be an object",
        ),
        (
            DocumentReason::TooDeep,
            "Definition nests deeper than 64 levels",
        ),
        (
            DocumentReason::NonFiniteValue,
            "Definition must contain finite JSON values",
        ),
        (DocumentReason::InvalidName, NAME_RULE),
        (
            DocumentReason::NameMismatch,
            "Definition name does not match source name",
        ),
        (
            DocumentReason::Malformed("node.network".to_string()),
            "Malformed node.network",
        ),
        (
            DocumentReason::MalformedFallback,
            "Malformed agent fallback",
        ),
        (
            DocumentReason::MalformedObserveRepos,
            "Malformed observe_repos: expected at most seven absolute paths",
        ),
        (DocumentReason::MalformedNode, "Malformed definition node"),
        (
            DocumentReason::MalformedConnection,
            "Malformed definition connection",
        ),
        (
            DocumentReason::MalformedReference,
            "Malformed workflow reference identity",
        ),
        (
            DocumentReason::Os("Permission denied".to_string()),
            "Permission denied",
        ),
    ];
    for (reason, expected) in table {
        assert_eq!(reason_message(&reason), expected);
        let failure = WorkflowFailure::Qualification(reason);
        assert_eq!(failure_message(&failure), expected);
    }
}
