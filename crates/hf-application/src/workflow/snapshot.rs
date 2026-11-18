//! Pure launch snapshots over an already pinned dependency tree.
use super::ports::DefinitionCodec;
use super::snapshot_scopes::{ResourceScopeFailure, validate_tree_scopes};
use super::validate::{Validator, WorkflowFailure};
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::refusal::Refusal;
use hf_domain::workflow::rule_shape::DefinitionRule;
use hf_domain::workflow::tree::DependencyTree;
use hf_domain::workflow::vocab::MAX_JSON_DEPTH;

#[derive(Clone, Debug, PartialEq)]
pub enum SnapshotFailure {
    Validation(WorkflowFailure),
    SourceNameMismatch,
    PinHashMismatch,
    RootMismatch,
    Scope(ResourceScopeFailure),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedSnapshot {
    pub definition: JsonObject,
    pub dependency_tree: DependencyTree,
}

fn depth_guard(definition: &JsonObject) -> Result<(), SnapshotFailure> {
    if definition.nests_beyond(MAX_JSON_DEPTH) {
        return Err(too_deep());
    }
    Ok(())
}

fn too_deep() -> SnapshotFailure {
    SnapshotFailure::Validation(WorkflowFailure::Refused(Refusal::new(
        DefinitionRule::TooDeep,
    )))
}

/// Copies the root, forces bounded context and applies orchestrator overrides before
/// validation. A backend change discards exactly the six backend-specific settings.
/// With nonempty overrides, the source must be a normalized stored root with an
/// object orchestrator. Malformed raw roots are not a supported Python parity path.
pub fn launch_definition(
    validator: &Validator<'_>,
    definition: &JsonObject,
    overrides: Option<&JsonObject>,
) -> Result<JsonObject, SnapshotFailure> {
    depth_guard(definition)?;
    let mut launched = definition.clone();
    launched.insert("context_delivery", Json::text("optimized_v1"));
    if let Some(overrides) = overrides.filter(|value| !value.is_empty()) {
        depth_guard(overrides)?;
        if let Some(orchestrator) = launched
            .get_mut("orchestrator")
            .and_then(Json::as_object_mut)
        {
            if overrides
                .get("backend")
                .is_some_and(|backend| Some(backend) != orchestrator.get("backend"))
            {
                for setting in [
                    "model",
                    "reasoning_effort",
                    "max_turns",
                    "background_idle_seconds",
                    "caller_approvals",
                    "caller_write_approvals",
                ] {
                    orchestrator.remove(setting);
                }
            }
            for (key, value) in overrides.iter() {
                orchestrator.insert(key, value.clone());
            }
        }
    }
    validator
        .validate(&Json::Object(launched))
        .map_err(SnapshotFailure::Validation)
}

/// Checks original pins in insertion order; normalization is used only for validation.
/// Inputs remain unchanged, including pin metadata and the source root definition.
/// The supplied tree must already be pinned for the launched definition, including
/// any orchestrator overrides; this function verifies pins rather than resolving them.
pub fn prepare_snapshot(
    validator: &Validator<'_>,
    codec: &dyn DefinitionCodec,
    expected_name: &str,
    definition: &JsonObject,
    overrides: Option<&JsonObject>,
    already_pinned_tree: &DependencyTree,
) -> Result<PreparedSnapshot, SnapshotFailure> {
    if definition.get("name").and_then(Json::as_str) != Some(expected_name) {
        return Err(SnapshotFailure::SourceNameMismatch);
    }
    let definition = launch_definition(validator, definition, overrides)?;
    for (_, entry) in &already_pinned_tree.workflows {
        depth_guard(&entry.definition)?;
        validator
            .validate(&Json::Object(entry.definition.clone()))
            .map_err(SnapshotFailure::Validation)?;
        if entry.definition_sha256 != codec.definition_sha256(&entry.definition) {
            return Err(SnapshotFailure::PinHashMismatch);
        }
    }
    let root = already_pinned_tree
        .entry(&already_pinned_tree.root_workflow_id)
        .filter(|entry| entry.name == expected_name)
        .ok_or(SnapshotFailure::RootMismatch)?;
    depth_guard(&definition)?;
    if root.definition_sha256 != codec.definition_sha256(&definition) {
        return Err(SnapshotFailure::RootMismatch);
    }
    for (_, entry) in &already_pinned_tree.workflows {
        if entry.revision.nests_beyond(MAX_JSON_DEPTH) {
            return Err(too_deep());
        }
    }
    let mut dependency_tree = already_pinned_tree.clone();
    for (_, entry) in &mut dependency_tree.workflows {
        entry
            .definition
            .insert("context_delivery", Json::text("optimized_v1"));
        entry.definition_sha256 = codec.definition_sha256(&entry.definition);
    }
    validate_tree_scopes(&dependency_tree).map_err(SnapshotFailure::Scope)?;
    Ok(PreparedSnapshot {
        definition,
        dependency_tree,
    })
}
