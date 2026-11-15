//! Normalization of saved definitions before validation, the read projection and warnings.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_support;

use hf_application::workflow::validate::migrate::{
    migrate_orchestrator_instructions, project_definition,
};
use hf_application::workflow::validate::warnings::definition_warnings;
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::refusal::Rule;
use hf_domain::workflow::rule_shape::DefinitionRule;
use workflow_support::FakeCodec;

fn object(entries: &[(&str, Json)]) -> JsonObject {
    JsonObject::from_entries(
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
    )
}

fn node(entries: &[(&str, Json)]) -> Json {
    Json::Object(object(entries))
}

fn start(prompt: Option<Json>) -> Json {
    let mut entries = vec![("id", Json::text("s")), ("type", Json::text("start"))];
    if let Some(value) = prompt {
        entries.push(("prompt", value));
    }
    node(&entries)
}

fn keys(map: &JsonObject) -> Vec<&str> {
    map.keys().collect()
}

#[test]
fn defaults_are_appended_in_order() {
    let migrated =
        migrate_orchestrator_instructions(&object(&[("name", Json::text("flow"))])).unwrap();
    assert_eq!(
        keys(&migrated),
        [
            "name",
            "orchestrator",
            "on_demand_nodes",
            "max_on_demand_calls"
        ]
    );
    let orchestrator = migrated
        .get("orchestrator")
        .and_then(Json::as_object)
        .unwrap();
    assert_eq!(orchestrator.get("backend"), Some(&Json::text("codex")));
    assert_eq!(orchestrator.get("instructions"), Some(&Json::text("")));
    assert_eq!(
        migrated.get("on_demand_nodes"),
        Some(&Json::Array(Vec::new()))
    );
    assert_eq!(migrated.get("max_on_demand_calls"), Some(&Json::int(5)));
}

#[test]
fn existing_values_are_not_replaced() {
    let input = object(&[
        (
            "orchestrator",
            node(&[
                ("backend", Json::text("claude")),
                ("instructions", Json::text("own")),
            ]),
        ),
        ("on_demand_nodes", Json::Array(vec![Json::int(1)])),
        ("max_on_demand_calls", Json::int(9)),
    ]);
    let migrated = migrate_orchestrator_instructions(&input).unwrap();
    assert_eq!(migrated, input);
}

#[test]
fn start_prompt_becomes_instructions() {
    let input = object(&[(
        "nodes",
        Json::Array(vec![start(Some(Json::text("build it")))]),
    )]);
    let migrated = migrate_orchestrator_instructions(&input).unwrap();
    let orchestrator = migrated
        .get("orchestrator")
        .and_then(Json::as_object)
        .unwrap();
    assert_eq!(
        orchestrator.get("instructions"),
        Some(&Json::text("build it"))
    );
    let nodes = migrated.get("nodes").and_then(Json::as_array).unwrap();
    assert!(!nodes[0].as_object().unwrap().contains_key("prompt"));
    assert!(
        input.get("nodes").and_then(Json::as_array).unwrap()[0]
            .as_object()
            .unwrap()
            .contains_key("prompt"),
        "the input is not changed"
    );
}

#[test]
fn own_instructions_win_over_the_prompt() {
    let input = object(&[
        ("orchestrator", node(&[("instructions", Json::text("own"))])),
        (
            "nodes",
            Json::Array(vec![start(Some(Json::text("prompt")))]),
        ),
    ]);
    let migrated = migrate_orchestrator_instructions(&input).unwrap();
    let orchestrator = migrated
        .get("orchestrator")
        .and_then(Json::as_object)
        .unwrap();
    assert_eq!(orchestrator.get("instructions"), Some(&Json::text("own")));
    assert!(
        !migrated.get("nodes").and_then(Json::as_array).unwrap()[0]
            .as_object()
            .unwrap()
            .contains_key("prompt")
    );
}

#[test]
fn non_text_start_prompt_is_refused() {
    for prompt in [
        Json::int(5),
        Json::Null,
        Json::Bool(true),
        Json::Array(Vec::new()),
    ] {
        let input = object(&[("nodes", Json::Array(vec![start(Some(prompt.clone()))]))]);
        let refusal = migrate_orchestrator_instructions(&input).unwrap_err();
        assert_eq!(
            refusal.rule,
            Rule::Definition(DefinitionRule::StartPromptNotText),
            "{prompt:?}"
        );
    }
}

#[test]
fn other_nodes_keep_their_prompt() {
    let agent = node(&[
        ("id", Json::text("a")),
        ("type", Json::text("agent")),
        ("prompt", Json::int(5)),
    ]);
    let input = object(&[("nodes", Json::Array(vec![agent.clone(), Json::int(3)]))]);
    let migrated = migrate_orchestrator_instructions(&input).unwrap();
    assert_eq!(
        migrated.get("nodes").and_then(Json::as_array).unwrap()[0],
        agent
    );
}

#[test]
fn non_object_orchestrator_is_left_for_validation() {
    let input = object(&[("orchestrator", Json::int(5))]);
    let migrated = migrate_orchestrator_instructions(&input).unwrap();
    assert_eq!(migrated, input);
}

#[test]
fn projection_adds_identity_and_execution_mode() {
    let agent = node(&[("id", Json::text("a")), ("type", Json::text("agent"))]);
    let fixed = node(&[
        ("id", Json::text("b")),
        ("type", Json::text("agent")),
        ("execution_mode", Json::text("prefer_subagent")),
    ]);
    let end = node(&[("id", Json::text("e")), ("type", Json::text("end"))]);
    let input = object(&[
        ("name", Json::text("flow")),
        ("nodes", Json::Array(vec![agent, fixed, end.clone()])),
    ]);
    let projected = project_definition(&FakeCodec, &input).unwrap();
    let id = projected.get("workflow_id").and_then(Json::as_str).unwrap();
    assert!(id.starts_with("legacy-"));
    let nodes = projected.get("nodes").and_then(Json::as_array).unwrap();
    assert_eq!(
        nodes[0].as_object().unwrap().get("execution_mode"),
        Some(&Json::text("headless"))
    );
    assert_eq!(
        nodes[1].as_object().unwrap().get("execution_mode"),
        Some(&Json::text("prefer_subagent"))
    );
    assert_eq!(nodes[2], end);
}

#[test]
fn projection_keeps_a_stored_id_and_propagates_refusals() {
    let input = object(&[
        ("name", Json::text("flow")),
        ("workflow_id", Json::text("kept")),
    ]);
    let projected = project_definition(&FakeCodec, &input).unwrap();
    assert_eq!(projected.get("workflow_id"), Some(&Json::text("kept")));
    let bad = object(&[("nodes", Json::Array(vec![start(Some(Json::int(1)))]))]);
    assert!(project_definition(&FakeCodec, &bad).is_err());
}

fn warn_for(nodes: Vec<Json>) -> Vec<String> {
    definition_warnings(&object(&[("nodes", Json::Array(nodes))]))
}

#[test]
fn warnings_name_agent_and_workflow_nodes_without_purpose() {
    let nodes = vec![
        start(None),
        node(&[("id", Json::text("a")), ("type", Json::text("agent"))]),
        node(&[("id", Json::text("w")), ("type", Json::text("workflow"))]),
        node(&[("id", Json::text("e")), ("type", Json::text("end"))]),
    ];
    assert_eq!(
        warn_for(nodes),
        [
            "Agent a has no purpose; context uses a bounded instructions excerpt",
            "Workflow w has no purpose; context uses a bounded instructions excerpt",
        ]
    );
}

#[test]
fn warnings_follow_python_truthiness_of_purpose() {
    let with = |purpose: Json| {
        node(&[
            ("id", Json::text("a")),
            ("type", Json::text("agent")),
            ("purpose", purpose),
        ])
    };
    for falsy in [
        Json::Null,
        Json::Bool(false),
        Json::text(""),
        Json::int(0),
        Json::Float(0.0),
        Json::Array(Vec::new()),
        Json::Object(JsonObject::new()),
    ] {
        assert_eq!(warn_for(vec![with(falsy.clone())]).len(), 1, "{falsy:?}");
    }
    for truthy in [
        Json::text("why"),
        Json::Bool(true),
        Json::int(2),
        Json::Float(0.5),
        Json::Array(vec![Json::int(0)]),
        node(&[("k", Json::Null)]),
    ] {
        assert!(
            warn_for(vec![with(truthy.clone())]).is_empty(),
            "{truthy:?}"
        );
    }
}

#[test]
fn warnings_tolerate_odd_nodes() {
    assert!(definition_warnings(&JsonObject::new()).is_empty());
    assert!(
        warn_for(vec![
            Json::int(1),
            Json::Null,
            node(&[("type", Json::int(2))])
        ])
        .is_empty()
    );
    let no_id = node(&[("type", Json::text("agent"))]);
    assert_eq!(
        warn_for(vec![no_id]),
        ["Agent  has no purpose; context uses a bounded instructions excerpt"]
    );
}
