//! The fields every connected node has, and Run workflow nodes.
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::rule_shape::NodeRule;
use hf_domain::workflow::text::{is_blank, length};
use hf_domain::workflow::vocab::NodeType;

use super::Validator;
use super::field::{Check, identifier, is_one_of, positive, refuse, refuse_for, timeout_valid};

/// The id and type of a validated node.
pub struct Checked {
    pub id: String,
    pub kind: NodeType,
}

fn default_position(index: usize) -> Json {
    let x = i64::try_from(index).unwrap_or(0) * 260 + 80;
    Json::Object(JsonObject::from_entries(Vec::from([
        ("x".to_string(), Json::int(x)),
        ("y".to_string(), Json::int(80)),
    ])))
}

/// A canvas coordinate: an integer or float, not a boolean, finite and nonnegative.
fn coordinate_valid(value: Option<&Json>) -> bool {
    match value {
        Some(Json::Int(number)) => !number.exceeds_float() && !number.is_negative(),
        Some(Json::Float(number)) => number.is_finite() && *number >= 0.0,
        _ => false,
    }
}

fn title_and_position_valid(node: &JsonObject) -> bool {
    let title = node
        .get("title")
        .is_some_and(|value| value.as_str().is_some());
    let position = node.get("position").and_then(Json::as_object);
    title
        && position
            .is_some_and(|map| coordinate_valid(map.get("x")) && coordinate_valid(map.get("y")))
}

fn purpose_valid(value: &Json) -> bool {
    value.as_str().is_some_and(|text| {
        !is_blank(text) && length(text) <= 500 && !text.contains('\r') && !text.contains('\n')
    })
}

impl Validator<'_> {
    /// One iteration of the node loop: normalizes `node` in place.
    pub(super) fn connected_node(
        &self,
        index: usize,
        node: &mut JsonObject,
        explicit: bool,
        seen: &[String],
    ) -> Check<Checked> {
        let id = identifier(node.get("id"))?;
        if seen.contains(&id) {
            return Err(refuse_for(NodeRule::DuplicateNode, &id));
        }
        let kind = node
            .get("type")
            .and_then(Json::as_str)
            .and_then(NodeType::parse)
            .ok_or_else(|| refuse_for(NodeRule::InvalidNodeType, &id))?;
        let boundary = matches!(kind, NodeType::ParallelStart | NodeType::ParallelEnd);
        if boundary && !explicit {
            return Err(refuse(NodeRule::ParallelNeedsExplicitRouting));
        }
        if kind == NodeType::ParallelStart {
            parallel_start_defaults(node)?;
        }
        if node
            .get("always_route")
            .is_some_and(|value| value.as_bool().is_none())
        {
            return Err(refuse(NodeRule::AlwaysRouteNotBool));
        }
        if node
            .get("purpose")
            .is_some_and(|value| !purpose_valid(value))
        {
            return Err(refuse_for(NodeRule::InvalidPurpose, &id));
        }
        node.set_default("title", Json::text(&id));
        node.set_default("position", default_position(index));
        if !title_and_position_valid(node) {
            return Err(refuse_for(NodeRule::InvalidTitleOrPosition, &id));
        }
        let mode = node
            .get("branch_mode")
            .cloned()
            .unwrap_or(Json::text("auto"));
        if !is_one_of(Some(&mode), &["auto", "choose_one", "all_matching"]) {
            return Err(refuse_for(NodeRule::InvalidBranchMode, &id));
        }
        let fixed = if explicit && kind != NodeType::ParallelStart {
            "choose_one"
        } else {
            "auto"
        };
        node.insert("branch_mode", Json::text(fixed));
        match kind {
            NodeType::Workflow => run_workflow_node(node, &id, explicit)?,
            NodeType::Agent => self.agent_node(node, &id)?,
            _ => {}
        }
        Ok(Checked { id, kind })
    }
}

fn parallel_start_defaults(node: &mut JsonObject) -> Check<()> {
    node.set_default("branch_selection", Json::text("all"));
    if !is_one_of(node.get("branch_selection"), &["all", "orchestrator"]) {
        return Err(refuse(NodeRule::InvalidBranchSelection));
    }
    node.set_default("selection_guidance", Json::text(""));
    if node
        .get("selection_guidance")
        .and_then(Json::as_str)
        .is_none()
    {
        return Err(refuse(NodeRule::SelectionGuidanceNotText));
    }
    Ok(())
}

fn workflow_ref_valid(node: &JsonObject) -> bool {
    node.get("workflow_ref")
        .and_then(Json::as_object)
        .is_some_and(|reference| {
            reference.keys().all(|key| key == "workflow_id")
                && reference
                    .get("workflow_id")
                    .and_then(Json::as_str)
                    .is_some_and(|id| !is_blank(id))
        })
}

fn run_workflow_node(node: &mut JsonObject, id: &str, explicit: bool) -> Check<()> {
    if !explicit {
        return Err(refuse_for(NodeRule::WorkflowNeedsExplicitRouting, id));
    }
    for field in [
        "agent",
        "role",
        "freedom",
        "network",
        "session_mode",
        "execution_mode",
        "max_context_questions",
    ] {
        if node.contains_key(field) {
            return Err(refuse(NodeRule::WorkflowRejectsField)
                .with_text(field)
                .with_text(id));
        }
    }
    if !workflow_ref_valid(node) {
        return Err(refuse_for(NodeRule::WorkflowRefRequired, id));
    }
    node.set_default("workflow_name", Json::text(""));
    if node.get("workflow_name").and_then(Json::as_str).is_none() {
        return Err(refuse_for(NodeRule::WorkflowNameNotText, id));
    }
    node.set_default("orchestrator_mode", Json::text("child"));
    if !is_one_of(node.get("orchestrator_mode"), &["child", "current"]) {
        return Err(refuse_for(NodeRule::InvalidOrchestratorMode, id));
    }
    node.set_default("child_session_policy", Json::text("agent_decides"));
    if !is_one_of(
        node.get("child_session_policy"),
        &["fresh", "resume", "agent_decides"],
    ) {
        return Err(refuse_for(NodeRule::InvalidChildSessionPolicy, id));
    }
    node.set_default("instructions", Json::text(""));
    if node.get("instructions").and_then(Json::as_str).is_none() {
        return Err(refuse_for(NodeRule::WorkflowInstructionsNotText, id));
    }
    node.set_default("optional", Json::Bool(false));
    if node.get("optional").and_then(Json::as_bool).is_none() {
        return Err(refuse_for(NodeRule::WorkflowOptionalNotBool, id));
    }
    node.insert("max_attempts", positive(node, "max_attempts", 3, 10000)?);
    if !timeout_valid(node) {
        return Err(refuse_for(NodeRule::WorkflowTimeoutInvalid, id));
    }
    Ok(())
}
