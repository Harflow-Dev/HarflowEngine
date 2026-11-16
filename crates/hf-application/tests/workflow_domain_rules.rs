//! Name grammars, Python string predicates, observed paths and the closed vocabularies.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use hf_domain::workflow::name::{is_identifier, is_resource_name, is_workflow_name};
use hf_domain::workflow::observe::{MAX_STORED, is_absolute};
use hf_domain::workflow::text::{is_blank, is_py_space, length};
use hf_domain::workflow::vocab::{
    EFFORTS, Freedom, MAX_CONNECTIONS, MAX_NESTING_DEPTH, MAX_NODES, NodeType, Role,
};

#[test]
fn identifier_grammar() {
    for good in [
        "a",
        "A",
        "0",
        "a.b",
        "a-b",
        "a_b",
        "a1.2-3_4",
        &"x".repeat(100),
    ] {
        assert!(is_identifier(good), "{good}");
    }
    for bad in [
        "",
        ".a",
        "-a",
        "_a",
        "a b",
        "a/b",
        "a\n",
        "é",
        "٣",
        "a\u{0}",
        &"x".repeat(101),
    ] {
        assert!(!is_identifier(bad), "{bad:?}");
    }
}

#[test]
fn workflow_name_grammar() {
    for good in ["a", "my flow", "a b c", "a  b", "a.b-c_d", &"x".repeat(100)] {
        assert!(is_workflow_name(good), "{good}");
    }
    for bad in [
        "",
        " a",
        "a ",
        "a  ",
        "-a",
        ".a",
        "a\tb",
        "a\n",
        "a/b",
        &"x".repeat(101),
        &format!("{} ", "x".repeat(99)),
    ] {
        assert!(!is_workflow_name(bad), "{bad:?}");
    }
}

#[test]
fn resource_name_grammar() {
    for good in ["a", "0", "db:main.1", "x_y-z", "a:b", &"a".repeat(64)] {
        assert!(is_resource_name(good), "{good}");
    }
    for bad in [
        "",
        "A",
        "aB",
        ":a",
        ".a",
        "-a",
        "_a",
        "a b",
        "a/b",
        "é",
        &"a".repeat(65),
    ] {
        assert!(!is_resource_name(bad), "{bad:?}");
    }
}

#[test]
fn python_whitespace_set() {
    for code in [
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x85, 0xa0, 0x1680, 0x2000,
        0x2005, 0x200a, 0x2028, 0x2029, 0x202f, 0x205f, 0x3000,
    ] {
        assert!(is_py_space(char::from_u32(code).unwrap()), "{code:#x}");
    }
    for code in [
        0x00, 0x08, 0x0e, 0x1b, 0x21, 0x61, 0x180e, 0x200b, 0x200c, 0x2060, 0xfeff,
    ] {
        assert!(!is_py_space(char::from_u32(code).unwrap()), "{code:#x}");
    }
}

#[test]
fn blank_and_length() {
    assert!(is_blank(""));
    assert!(is_blank(" \t\n\u{a0}\u{3000}"));
    assert!(!is_blank(" x "));
    assert!(!is_blank("\u{200b}"));
    assert_eq!(length(""), 0);
    assert_eq!(length("abc"), 3);
    assert_eq!(length("é"), 1);
    assert_eq!(length("\u{1f600}"), 1);
    assert_eq!(length("e\u{301}"), 2);
}

#[test]
fn absolute_paths_and_bounds() {
    assert!(is_absolute("/"));
    assert!(is_absolute("/repo/a"));
    assert!(!is_absolute(""));
    assert!(!is_absolute("repo"));
    assert!(!is_absolute("./repo"));
    assert!(!is_absolute("~/repo"));
    assert_eq!(MAX_STORED, 7);
}

#[test]
fn vocabularies_round_trip() {
    for freedom in Freedom::ALL {
        assert_eq!(Freedom::parse(freedom.as_str()), Some(freedom));
    }
    assert_eq!(Freedom::parse("Publish"), None);
    assert_eq!(Freedom::parse(""), None);
    for (role, freedom) in [
        (Role::Planning, Freedom::ReadOnly),
        (Role::Review, Freedom::ReadOnly),
        (Role::Implementation, Freedom::WriteInRepo),
        (Role::Task, Freedom::Publish),
    ] {
        assert_eq!(Role::parse(role.as_str()), Some(role));
        assert_eq!(role.default_freedom(), freedom);
    }
    assert_eq!(Role::parse("boss"), None);
    for text in [
        "start",
        "agent",
        "join",
        "end",
        "parallel_start",
        "parallel_end",
        "workflow",
    ] {
        assert_eq!(NodeType::parse(text).map(NodeType::as_str), Some(text));
    }
    assert_eq!(NodeType::parse("Start"), None);
}

#[test]
fn executable_nodes_and_constants() {
    for (kind, executable) in [
        (NodeType::Agent, true),
        (NodeType::Workflow, true),
        (NodeType::Start, false),
        (NodeType::End, false),
        (NodeType::Join, false),
        (NodeType::ParallelStart, false),
        (NodeType::ParallelEnd, false),
    ] {
        assert_eq!(kind.is_executable(), executable, "{kind:?}");
    }
    assert_eq!(EFFORTS, ["low", "medium", "high", "xhigh"]);
    assert_eq!(MAX_NESTING_DEPTH, 4);
    assert_eq!(MAX_NODES, 1000);
    assert_eq!(MAX_CONNECTIONS, 4000);
}
