//! Pure dependency resolution over one caller-owned, locked catalog snapshot.
use super::identity::workflow_identity;
use super::ports::DefinitionCodec;
use super::validate::{Validator, WorkflowFailure};
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::refusal::{DependencyRefusal, DependencyRule, Refusal};
use hf_domain::workflow::rule_shape::DefinitionRule;
use hf_domain::workflow::tree::{DependencyTree, TreeEdge, TreeEntry, access_summary, text};
use hf_domain::workflow::vocab::{MAX_JSON_DEPTH, MAX_NESTING_DEPTH};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub enum ReferenceFailure {
    Validation(WorkflowFailure),
    Dependency(DependencyRefusal),
}

pub(crate) fn refuse(rule: DependencyRule, detail: &str) -> DependencyRefusal {
    DependencyRefusal {
        refusal: Refusal::new(rule).with_text(detail),
        missing_references: Vec::new(),
        truncated: false,
    }
}

/// Index in source order, excluding the save candidate's old slot. Later equal copies win.
/// Input is borrowed and depth-checked before any recursive hashing or cloning.
pub fn indexed_definitions(
    codec: &dyn DefinitionCodec,
    definitions: &[JsonObject],
    substitute_name: Option<&str>,
) -> Result<HashMap<String, JsonObject>, ReferenceFailure> {
    let mut index: HashMap<String, JsonObject> = HashMap::new();
    let mut hashes = HashMap::new();
    for definition in definitions {
        if substitute_name == Some(text(definition, "name")) {
            continue;
        }
        check_depth(definition)?;
        let id = workflow_identity(codec, definition);
        let hash = codec.definition_sha256(definition);
        if hashes.get(&id).is_some_and(|previous| previous != &hash) {
            return Err(ReferenceFailure::Dependency(refuse(
                DependencyRule::Duplicate,
                &id,
            )));
        }
        hashes.insert(id.clone(), hash);
        index.insert(id, definition.clone());
    }
    Ok(index)
}

struct Frame<'a> {
    id: String,
    definition: &'a JsonObject,
    cursor: usize,
    depth: usize,
}
struct Resolution<'a> {
    codec: &'a dyn DefinitionCodec,
    validator: &'a Validator<'a>,
    index: &'a HashMap<String, JsonObject>,
    tree: DependencyTree,
    valid: HashSet<String>,
    hashes: HashMap<String, String>,
    completed: HashMap<String, usize>,
    edges: HashSet<(String, String, String, String)>,
}

impl<'a> Resolution<'a> {
    fn enter(
        &mut self,
        definition: &'a JsonObject,
        depth: usize,
        path: &[Frame<'_>],
    ) -> Result<bool, ReferenceFailure> {
        check_depth(definition)?;
        let id = workflow_identity(self.codec, definition);
        // Root and catalog may share an identity but differ in bytes: validate before hash comparison.
        let hash = if self
            .index
            .get(&id)
            .is_some_and(|indexed| std::ptr::eq(indexed, definition))
        {
            self.hashes
                .entry(id.clone())
                .or_insert_with(|| self.codec.definition_sha256(definition))
                .clone()
        } else {
            self.codec.definition_sha256(definition)
        };
        let cache_key = format!("{id}:{hash}");
        if self.valid.insert(cache_key) {
            self.validator
                .validate(&Json::Object(definition.clone()))
                .map_err(ReferenceFailure::Validation)?;
        }
        if depth > MAX_NESTING_DEPTH {
            let mut names: Vec<&str> = path
                .iter()
                .map(|frame| self.pinned_name(&frame.id))
                .collect();
            // Python's path already includes the incoming target before it is pinned.
            names.push(self.pinned_name(&id));
            names.push(text(definition, "name"));
            return Err(ReferenceFailure::Dependency(refuse(
                DependencyRule::Nesting,
                &names.join(" > "),
            )));
        }
        if let Some(indexed) = self.index.get(&id) {
            let indexed_hash = self
                .hashes
                .entry(id.clone())
                .or_insert_with(|| self.codec.definition_sha256(indexed));
            if indexed_hash != &hash {
                return Err(ReferenceFailure::Dependency(refuse(
                    DependencyRule::Duplicate,
                    &id,
                )));
            }
        }
        if self.completed.get(&id).is_some_and(|done| *done >= depth) {
            return Ok(false);
        }
        let entry = TreeEntry {
            name: text(definition, "name").to_string(),
            revision: definition.get("revision").cloned().unwrap_or(Json::int(0)),
            definition_sha256: hash,
            definition: definition.clone(),
        };
        if let Some((_, previous)) = self.tree.workflows.iter_mut().find(|(key, _)| key == &id) {
            *previous = entry;
        } else {
            self.tree.workflows.push((id.clone(), entry));
        }
        if let Some((_, previous)) = self.tree.access.iter_mut().find(|(key, _)| key == &id) {
            *previous = access_summary(definition, previous.depth.max(depth));
        } else {
            self.tree
                .access
                .push((id, access_summary(definition, depth)));
        }
        Ok(true)
    }

    fn pinned_name<'b>(&'b self, id: &'b str) -> &'b str {
        self.tree
            .entry(id)
            .map(|entry| entry.name.as_str())
            .filter(|name| !name.is_empty())
            .unwrap_or(id)
    }

    fn target(
        &mut self,
        frame: &Frame<'a>,
        node: &JsonObject,
        path: &[Frame<'_>],
    ) -> Result<&'a JsonObject, ReferenceFailure> {
        let target_id = node
            .get("workflow_ref")
            .and_then(Json::as_object)
            .map(|reference| text(reference, "workflow_id"))
            .unwrap_or("");
        if hf_domain::workflow::text::is_blank(target_id) {
            return Err(ReferenceFailure::Dependency(refuse(
                DependencyRule::MissingIdentity,
                text(node, "id"),
            )));
        }
        if let Some(position) = path.iter().position(|item| item.id == target_id) {
            let mut names: Vec<&str> = path[position..]
                .iter()
                .map(|item| text(item.definition, "name"))
                .collect();
            names.push(names[0]);
            return Err(ReferenceFailure::Dependency(refuse(
                DependencyRule::Cycle,
                &names.join(" > "),
            )));
        }
        let Some(target) = self.index.get(target_id) else {
            let mut error = refuse(DependencyRule::Unknown, target_id);
            error.refusal = error.refusal.with_text(text(node, "id"));
            return Err(ReferenceFailure::Dependency(error));
        };
        let mode = node
            .get("orchestrator_mode")
            .and_then(Json::as_str)
            .unwrap_or("child");
        let key = (
            frame.id.clone(),
            text(node, "id").to_string(),
            target_id.to_string(),
            mode.to_string(),
        );
        if self.edges.insert(key.clone()) {
            self.tree.edges.push(TreeEdge {
                from: key.0,
                node_id: key.1,
                to: key.2,
                orchestrator_mode: key.3,
            });
        }
        Ok(target)
    }

    fn walk(&mut self, root: &'a JsonObject) -> Result<(), ReferenceFailure> {
        self.enter(root, 1, &[])?;
        let mut stack = Vec::from([Frame {
            id: workflow_identity(self.codec, root),
            definition: root,
            cursor: 0,
            depth: 1,
        }]);
        while let Some(frame) = stack.last() {
            let node = frame
                .definition
                .get("nodes")
                .and_then(Json::as_array)
                .and_then(|items| items.get(frame.cursor))
                .and_then(Json::as_object);
            let Some(node) = node else {
                if let Some(done) = stack.pop() {
                    self.completed
                        .entry(done.id)
                        .and_modify(|depth| *depth = (*depth).max(done.depth))
                        .or_insert(done.depth);
                }
                continue;
            };
            let depth = frame.depth + 1;
            if text(node, "type") == "workflow" {
                let target = self.target(frame, node, &stack)?;
                let expand = self.enter(target, depth, &stack)?;
                if let Some(parent) = stack.last_mut() {
                    parent.cursor += 1;
                }
                if expand {
                    stack.push(Frame {
                        id: workflow_identity(self.codec, target),
                        definition: target,
                        cursor: 0,
                        depth,
                    });
                }
            } else if let Some(parent) = stack.last_mut() {
                parent.cursor += 1;
            }
        }
        Ok(())
    }
}

/// Validate the root first, index the snapshot, then pin in depth-first occurrence order.
pub fn resolve_dependencies(
    validator: &Validator<'_>,
    codec: &dyn DefinitionCodec,
    raw: &Json,
    definitions: &[JsonObject],
    substitute_name: Option<&str>,
) -> Result<DependencyTree, ReferenceFailure> {
    let root = validator
        .validate(raw)
        .map_err(ReferenceFailure::Validation)?;
    let index = indexed_definitions(codec, definitions, substitute_name)?;
    let root_id = workflow_identity(codec, &root);
    let mut resolution = Resolution {
        codec,
        validator,
        index: &index,
        tree: DependencyTree {
            root_workflow_id: root_id,
            workflows: Vec::new(),
            edges: Vec::new(),
            access: Vec::new(),
        },
        valid: HashSet::new(),
        hashes: HashMap::new(),
        completed: HashMap::new(),
        edges: HashSet::new(),
    };
    match resolution.walk(&root) {
        Err(ReferenceFailure::Dependency(mut error)) => {
            (error.missing_references, error.truncated) =
                super::diagnostics::missing_references(validator, codec, &root, &index);
            Err(ReferenceFailure::Dependency(error))
        }
        Err(other) => Err(other),
        Ok(()) => Ok(resolution.tree),
    }
}

/// Restrict an existing pin without resolving or reinterpreting definitions.
pub fn subtree(
    tree: &DependencyTree,
    workflow_id: &str,
) -> Result<DependencyTree, DependencyRefusal> {
    tree.subtree(workflow_id)
        .ok_or_else(|| refuse(DependencyRule::NotInTree, workflow_id))
}

fn check_depth(definition: &JsonObject) -> Result<(), ReferenceFailure> {
    if definition.nests_beyond(MAX_JSON_DEPTH) {
        return Err(ReferenceFailure::Validation(WorkflowFailure::Refused(
            Refusal::new(DefinitionRule::TooDeep),
        )));
    }
    Ok(())
}
