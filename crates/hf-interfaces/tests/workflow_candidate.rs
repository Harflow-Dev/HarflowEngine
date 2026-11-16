//! Agent candidates: settings, grants, capabilities and fallbacks, through the validator.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use hf_domain::workflow::json::Json;

fn node_agent(settings: &str) -> hf_domain::workflow::json::JsonObject {
    let definition = accepted(&with_agent(settings));
    at(&definition, "nodes.1.agent")
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

fn refused_agent(settings: &str) -> String {
    refused(&with_agent(settings))
}

#[test]
fn unknown_settings_sorted() {
    assert_eq!(
        refused_agent(r#""zeta":1,"alpha":2,"backend":"codex""#),
        "Unknown agent settings: ['alpha', 'zeta']"
    );
    assert_eq!(
        candidate(r#"{"backend":"codex","x":1}"#, true).unwrap_err(),
        "Unknown agent settings: ['x']"
    );
}

#[test]
fn defaults_are_filled() {
    let agent = node_agent(r#""backend":"codex""#);
    let order: Vec<&str> = agent.keys().collect();
    assert_eq!(order, ["backend", "caller_approvals", "fallbacks"]);
    assert_eq!(agent.get("caller_approvals"), Some(&Json::Bool(false)));
    assert_eq!(agent.get("fallbacks"), Some(&Json::Array(Vec::new())));
}

#[test]
fn conversation_profile_values() {
    for profile in ["native", "shipped-medium35-high"] {
        let agent = node_agent(&format!(
            r#""backend":"codex","conversation_profile":"{profile}""#
        ));
        assert_eq!(
            agent.get("conversation_profile"),
            Some(&Json::text(profile))
        );
    }
    let message = "conversation_profile must be native or shipped-medium35-high";
    assert_eq!(refused_agent(r#""conversation_profile":"fast""#), message);
    assert_eq!(refused_agent(r#""conversation_profile":5"#), message);
}

#[test]
fn backend_lookup() {
    let known = "['antigravity', 'claude', 'codex', 'cursor', 'opencode', 'vibe']";
    assert_eq!(
        refused_agent(r#""backend":"nope""#),
        format!("unknown backend 'nope'; expected one of {known}")
    );
    assert_eq!(
        refused_agent(r#""backend":5"#),
        format!("unknown backend 5; expected one of {known}")
    );
    assert_eq!(
        refused_agent(r#""backend":null"#),
        format!("unknown backend None; expected one of {known}")
    );
}

#[test]
fn empty_agent_object_unknown_backend() {
    let message = refused(&agent(r#""agent":{}"#));
    assert!(
        message.starts_with("unknown backend ''; expected one of ["),
        "{message}"
    );
}

#[test]
fn null_agent_refused() {
    assert_eq!(
        refused(&agent(r#""agent":null"#)),
        "Agent must be an object"
    );
    assert_eq!(
        refused(&agent(r#""agent":"codex""#)),
        "Agent must be an object"
    );
    assert_eq!(refused(&agent(r#""agent":[]"#)), "Agent must be an object");
}

#[test]
fn absent_agent_defaults_to_codex() {
    let definition = accepted(&agent(""));
    assert_eq!(
        at(&definition, "nodes.1.agent.backend"),
        Some(&Json::text("codex"))
    );
}

#[test]
fn fallbacks_shape() {
    assert_eq!(
        refused_agent(r#""fallbacks":{}"#),
        "Fallbacks must be an ordered array"
    );
    assert_eq!(
        refused_agent(r#""fallbacks":null"#),
        "Fallbacks must be an ordered array"
    );
    let nested =
        r#""backend":"codex","fallbacks":[{"backend":"claude","fallbacks":[{"backend":"codex"}]}]"#;
    assert_eq!(
        refused_agent(nested),
        "Fallbacks must be an ordered flat list"
    );
    assert_eq!(
        refused_agent(r#""backend":"codex","fallbacks":[{"backend":"nope"}]"#)
            .split(';')
            .next(),
        Some("unknown backend 'nope'")
    );
    assert_eq!(
        refused_agent(r#""backend":"codex","fallbacks":[5]"#),
        "Agent must be an object"
    );
    let agent = node_agent(r#""backend":"codex","fallbacks":[{"backend":"claude"}]"#);
    let Some(Json::Array(items)) = agent.get("fallbacks") else {
        panic!("fallbacks missing");
    };
    let fallback = items[0].as_object().unwrap();
    assert_eq!(fallback.get("max_turns"), Some(&Json::int(100)));
    assert_eq!(fallback.get("fallbacks"), Some(&Json::Array(Vec::new())));
}

#[test]
fn capability_refusals_suppressed_by_fallbacks() {
    let message = refused_agent(r#""backend":"codex","max_turns":5"#);
    assert!(
        message.starts_with("the codex CLI has no turn cap, so max_turns=5 cannot be honoured")
    );
    let rescued = r#""backend":"codex","max_turns":5,"fallbacks":[{"backend":"claude"}]"#;
    assert_eq!(node_agent(rescued).get("max_turns"), Some(&Json::int(5)));
    let own =
        refused_agent(r#""backend":"claude","fallbacks":[{"backend":"codex","max_turns":5}]"#);
    assert!(own.starts_with("the codex CLI has no turn cap"), "{own}");
}

#[test]
fn caller_approvals_rules() {
    assert_eq!(
        refused_agent(r#""caller_approvals":"yes""#),
        "caller_approvals must be a boolean"
    );
    assert_eq!(
        refused_agent(r#""caller_approvals":1"#),
        "caller_approvals must be a boolean"
    );
    let unqualified = "Caller approvals require a qualified uncapped backend";
    assert_eq!(
        refused_agent(r#""backend":"codex","caller_approvals":true"#),
        unqualified
    );
    assert_eq!(
        refused_agent(r#""backend":"opencode","caller_approvals":true"#),
        unqualified
    );
    assert_eq!(
        refused_agent(r#""backend":"claude","caller_approvals":true,"max_turns":5"#),
        unqualified
    );
    let agent = node_agent(r#""backend":"claude","caller_approvals":true"#);
    assert_eq!(agent.get("caller_approvals"), Some(&Json::Bool(true)));
    assert_eq!(agent.get("max_turns"), None);
}

#[test]
fn caller_write_approvals_rules() {
    let strict = "caller_write_approvals must be a boolean";
    assert_eq!(refused_agent(r#""caller_write_approvals":null"#), strict);
    assert_eq!(refused_agent(r#""caller_write_approvals":0"#), strict);
    assert_eq!(
        refused_agent(r#""caller_write_approvals":true"#),
        "caller_write_approvals requires caller_approvals"
    );
    let kept =
        node_agent(r#""backend":"claude","caller_approvals":true,"caller_write_approvals":true"#);
    assert_eq!(kept.get("caller_write_approvals"), Some(&Json::Bool(true)));
    let dropped = node_agent(r#""caller_write_approvals":false"#);
    assert_eq!(dropped.get("caller_write_approvals"), None);
}

#[test]
fn group_report_output_rules() {
    assert_eq!(
        refused_agent(r#""group_report_output":"x""#),
        "group_report_output must be a boolean"
    );
    assert_eq!(
        refused_agent(r#""group_report_output":true"#),
        "Node a: group_report_output requires read_only"
    );
    let report =
        agent(r#""freedom":"read_only","agent":{"backend":"codex","group_report_output":true}"#);
    assert_eq!(
        at(&accepted(&report), "nodes.1.agent.group_report_output"),
        Some(&Json::Bool(true))
    );
    let approvals = agent(
        r#""freedom":"read_only","agent":{"backend":"claude","caller_approvals":true,"group_report_output":true}"#,
    );
    assert_eq!(
        refused(&approvals),
        "group_report_output cannot be combined with caller approvals"
    );
    let orchestrator = top(r#""orchestrator":{"group_report_output":true,"backend":"codex"}"#);
    assert_eq!(
        refused(&orchestrator),
        "group_report_output belongs to workflow agent nodes, not orchestrators or builders"
    );
    assert!(
        accepted(&top(
            r#""orchestrator":{"group_report_output":false,"backend":"codex"}"#
        ))
        .contains_key("orchestrator")
    );
}

#[test]
fn git_https_rewrite_is_boolean() {
    assert_eq!(
        refused_agent(r#""git_https_rewrite":"on""#),
        "git_https_rewrite must be a boolean"
    );
    assert_eq!(
        refused_agent(r#""git_https_rewrite":null"#),
        "git_https_rewrite must be a boolean"
    );
    assert_eq!(
        node_agent(r#""git_https_rewrite":true"#).get("git_https_rewrite"),
        Some(&Json::Bool(true))
    );
}
