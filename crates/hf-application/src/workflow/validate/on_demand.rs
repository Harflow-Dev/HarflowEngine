//! On-demand nodes: standalone workers an orchestrator may call.
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::rule_shape::{NodeRule, OnDemandRule};
use hf_domain::workflow::text::is_blank;
use hf_domain::workflow::vocab::Freedom;

use super::Validator;
use super::field::{Check, identifier, is_one_of, network_valid, refuse, refuse_for};

const ALLOWED: [&str; 10] = [
    "id",
    "title",
    "instructions",
    "call_condition",
    "agent",
    "freedom",
    "network",
    "session_mode",
    "execution_mode",
    "resources",
];

fn default_agent() -> Json {
    Json::Object(JsonObject::from_entries(Vec::from([(
        "backend".to_string(),
        Json::text("codex"),
    )])))
}

impl Validator<'_> {
    /// `validate_nodes(raw, connected_ids)`.
    pub(super) fn on_demand_nodes(&self, raw: &Json, connected: &[String]) -> Check<Vec<Json>> {
        let Json::Array(items) = raw else {
            return Err(refuse(OnDemandRule::OnDemandNotArray));
        };
        let mut ids: Vec<String> = connected.to_vec();
        let mut result = Vec::new();
        for item in items {
            let node = item
                .as_object()
                .filter(|map| map.keys().all(|key| ALLOWED.contains(&key)))
                .ok_or_else(|| refuse(OnDemandRule::OnDemandKeys))?;
            let mut node = node.clone();
            let id = identifier(node.get("id"))?;
            if ids.contains(&id) {
                return Err(refuse_for(NodeRule::DuplicateNode, &id));
            }
            ids.push(id.clone());
            self.on_demand_node(&mut node, &id)?;
            result.push(Json::Object(node));
        }
        Ok(result)
    }

    fn on_demand_node(&self, node: &mut JsonObject, id: &str) -> Check<()> {
        node.set_default("title", Json::text(id));
        node.set_default("call_condition", Json::text(""));
        let instructions = node.get("instructions").and_then(Json::as_str);
        if instructions.is_none_or(is_blank) {
            return Err(refuse_for(OnDemandRule::OnDemandInstructions, id));
        }
        let texts = ["title", "call_condition"]
            .iter()
            .all(|key| node.get(key).and_then(Json::as_str).is_some());
        if !texts {
            return Err(refuse_for(OnDemandRule::OnDemandTitleOrCondition, id));
        }
        let agent = node.get("agent").cloned().unwrap_or_else(default_agent);
        node.insert("agent", Json::Object(self.candidate(&agent, true)?));
        node.set_default("freedom", Json::text("read_only"));
        node.set_default("session_mode", Json::text("agent_decides"));
        node.set_default("execution_mode", Json::text("headless"));
        let freedom = node
            .get("freedom")
            .and_then(Json::as_str)
            .and_then(Freedom::parse);
        if freedom.is_none() || !network_valid(node) {
            return Err(refuse_for(OnDemandRule::OnDemandFreedomOrNetwork, id));
        }
        self.check_node_grants(node, id)?;
        if !is_one_of(
            node.get("session_mode"),
            &["agent_decides", "fresh", "resume"],
        ) {
            return Err(refuse_for(OnDemandRule::OnDemandSessionMode, id));
        }
        if !is_one_of(node.get("execution_mode"), &["headless", "prefer_subagent"]) {
            return Err(refuse_for(OnDemandRule::OnDemandExecutionMode, id));
        }
        Ok(())
    }
}
