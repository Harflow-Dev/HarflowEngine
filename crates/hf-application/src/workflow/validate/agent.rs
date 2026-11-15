//! Agent nodes: role, candidate, grants and Git preparation.
use hf_domain::workflow::git;
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::rule_shape::NodeRule;
use hf_domain::workflow::vocab::{Freedom, Role};

use super::Validator;
use super::field::{Check, is_one_of, network_valid, positive, refuse, refuse_for, timeout_valid};

fn default_agent() -> Json {
    Json::Object(JsonObject::from_entries(Vec::from([(
        "backend".to_string(),
        Json::text("codex"),
    )])))
}

fn set_bool_default(node: &mut JsonObject, key: &str, value: bool) -> bool {
    node.set_default(key, Json::Bool(value));
    node.get(key).and_then(Json::as_bool).is_some()
}

impl Validator<'_> {
    /// The agent-specific part of the node loop.
    pub(super) fn agent_node(&self, node: &mut JsonObject, id: &str) -> Check<()> {
        if !set_bool_default(node, "optional", false) {
            return Err(refuse(NodeRule::AgentOptionalNotBool));
        }
        node.set_default("role", Json::text("task"));
        let role = node
            .get("role")
            .and_then(Json::as_str)
            .and_then(Role::parse)
            .ok_or_else(|| refuse_for(NodeRule::InvalidRole, id))?;
        if role == Role::Planning {
            for field in ["require_technical_plan", "require_tasks"] {
                if !set_bool_default(node, field, true) {
                    return Err(refuse(NodeRule::PlanningFlagNotBool).with_text(field));
                }
            }
        }
        let agent = node.get("agent").cloned().unwrap_or_else(default_agent);
        node.insert("agent", Json::Object(self.candidate(&agent, true)?));
        node.set_default("instructions", Json::text(""));
        if node.get("instructions").and_then(Json::as_str).is_none() || !network_valid(node) {
            return Err(refuse_for(NodeRule::InvalidInstructionsOrNetwork, id));
        }
        node.set_default("freedom", Json::text(role.default_freedom().as_str()));
        let freedom = node
            .get("freedom")
            .and_then(Json::as_str)
            .and_then(Freedom::parse)
            .ok_or_else(|| refuse(NodeRule::InvalidFreedom))?;
        if role == Role::Implementation && freedom == Freedom::ReadOnly {
            return Err(refuse(NodeRule::ImplementationReadOnly));
        }
        self.check_node_grants(node, id)?;
        git::validate_node(node, id)?;
        node.set_default("execution_mode", Json::text("headless"));
        if !is_one_of(node.get("execution_mode"), &["headless", "prefer_subagent"]) {
            return Err(refuse_for(NodeRule::InvalidExecutionMode, id));
        }
        node.set_default("session_mode", Json::text("agent_decides"));
        if !is_one_of(
            node.get("session_mode"),
            &["resume", "fresh", "agent_decides", "continue_previous"],
        ) {
            return Err(refuse_for(NodeRule::InvalidSessionMode, id));
        }
        node.insert("max_attempts", positive(node, "max_attempts", 3, 10000)?);
        if !timeout_valid(node) {
            return Err(refuse(NodeRule::TimeoutInvalid));
        }
        node.insert(
            "max_context_questions",
            positive(node, "max_context_questions", 10, 100)?,
        );
        Ok(())
    }

    /// `check_node_grants`: every candidate against the node's own freedom.
    pub(super) fn check_node_grants(&self, node: &JsonObject, label: &str) -> Check<()> {
        let freedom = node
            .get("freedom")
            .and_then(Json::as_str)
            .and_then(Freedom::parse)
            .unwrap_or(Freedom::Publish);
        let Some(agent) = node.get("agent").and_then(Json::as_object) else {
            return Ok(());
        };
        let fallbacks = agent
            .get("fallbacks")
            .and_then(Json::as_array)
            .into_iter()
            .flatten();
        let candidates = std::iter::once(agent).chain(fallbacks.filter_map(Json::as_object));
        for candidate in candidates {
            let report = candidate.get("group_report_output").and_then(Json::as_bool) == Some(true);
            if report && freedom != Freedom::ReadOnly {
                return Err(refuse_for(NodeRule::GroupReportNeedsReadOnly, label));
            }
            let patterns: Vec<String> = candidate
                .get("allowed_commands")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect();
            let backend = candidate
                .get("backend")
                .and_then(Json::as_str)
                .unwrap_or("");
            if !patterns.is_empty()
                && let Err(reason) = self
                    .catalog
                    .check_allowed_commands(backend, &patterns, freedom)
            {
                return Err(refuse(NodeRule::AllowedCommandsRefused)
                    .with_text(label)
                    .with_text(backend)
                    .with_text(&reason));
            }
        }
        Ok(())
    }
}
