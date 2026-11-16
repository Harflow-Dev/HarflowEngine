//! Save a definition atomically after resolution and caller-supplied authority admission.
use super::catalog::{CatalogFailure, check_name, document_error, project_all, qualify_named};
use super::identity::{persist_workflow_id, workflow_identity};
use super::ports::{
    Clock, DefinitionCodec, DefinitionStore, DefinitionTxn, IdSource, SaveAuthority,
};
use super::references::resolve_dependencies;
use super::save_defaults::execution_modes;
use super::validate::{Validator, WorkflowFailure};
use hf_domain::workflow::document::qualify_stored;
use hf_domain::workflow::json::{Json, JsonInt, JsonObject};
use hf_domain::workflow::refusal::{DocumentReason, Refusal};
use hf_domain::workflow::rule_shape::DefinitionRule;
use hf_domain::workflow::tree::DependencyTree;
use hf_domain::workflow::vocab::{INT_DIGIT_LIMIT, MAX_JSON_DEPTH};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SaveExpectations {
    pub revision: Option<JsonInt>,
    pub workflow_id: Option<String>,
}

/// All effects needed by save are explicit, caller-owned ports.
pub struct DefinitionSaver<'a> {
    pub validator: &'a Validator<'a>,
    pub codec: &'a dyn DefinitionCodec,
    pub ids: &'a dyn IdSource,
    pub clock: &'a dyn Clock,
    pub authority: &'a dyn SaveAuthority,
}

impl DefinitionSaver<'_> {
    pub fn save(
        &self,
        store: &impl DefinitionStore,
        name: &str,
        raw: &JsonObject,
        expected: &SaveExpectations,
    ) -> Result<JsonObject, CatalogFailure> {
        check_name(name)?;
        // Refuse depth before cloning caller-owned JSON.
        if raw.nests_beyond(MAX_JSON_DEPTH) {
            return Err(CatalogFailure::Validation(WorkflowFailure::Refused(
                Refusal::new(DefinitionRule::TooDeep),
            )));
        }
        let mut input = raw.clone();
        input.insert("name", Json::text(name));
        let mut definition = self
            .validator
            .validate(&Json::Object(input))
            .map_err(CatalogFailure::Validation)?;
        definition.remove("workflow_id");
        definition.insert("execution_contract", Json::text("delegation"));
        let txn = store.begin(name).map_err(CatalogFailure::Document)?;
        let old = txn.current();
        if let Some(old) = old {
            qualify_named(old, name)?;
        }
        let previous = old.map(|source| &source.definition);
        self.check_identity(previous, expected)?;
        definition.insert("context_delivery", Json::text("optimized_v1"));
        execution_modes(&mut definition, raw, previous);
        let revision = next_revision(name, previous, expected)?;
        definition.insert("revision", Json::Int(revision));
        definition.insert(
            "workflow_id",
            Json::Str(persist_workflow_id(self.codec, self.ids, previous)),
        );
        let sources = txn.list().map_err(CatalogFailure::Document)?;
        let catalog = project_all(self.codec, &sources)?;
        let tree = resolve_dependencies(
            self.validator,
            self.codec,
            &Json::Object(definition.clone()),
            &catalog,
            Some(name),
        )?;
        pin_names(&mut definition, &tree);
        self.authority
            .admit(&tree)
            .map_err(CatalogFailure::Authority)?;
        definition.insert("updated_at", Json::Float(self.clock.unix_seconds()));
        qualify_stored(&definition, Some(name))
            .map_err(|reason| CatalogFailure::Validation(WorkflowFailure::Qualification(reason)))?;
        txn.replace(&definition).map_err(CatalogFailure::Document)?;
        Ok(definition)
    }

    fn check_identity(
        &self,
        previous: Option<&JsonObject>,
        expected: &SaveExpectations,
    ) -> Result<(), CatalogFailure> {
        if let Some(expected) = &expected.workflow_id {
            let current = previous.map(|old| workflow_identity(self.codec, old));
            if current.as_ref() != Some(expected) {
                return Err(CatalogFailure::StaleIdentity {
                    expected: expected.clone(),
                    current,
                });
            }
        }
        Ok(())
    }
}

fn next_revision(
    name: &str,
    previous: Option<&JsonObject>,
    expected: &SaveExpectations,
) -> Result<JsonInt, CatalogFailure> {
    let revision = match previous {
        Some(old) => old
            .get("revision")
            .and_then(Json::as_int)
            .cloned()
            .ok_or_else(|| {
                document_error(
                    name,
                    DocumentReason::Malformed("definition.revision".to_string()),
                )
            })?,
        None => JsonInt::from_i64(0),
    };
    if expected.revision.as_ref() != Some(&revision)
        && (previous.is_some()
            || expected
                .revision
                .as_ref()
                .is_some_and(|value| !value.is_zero()))
    {
        return Err(CatalogFailure::StaleRevision {
            expected: expected.revision.clone(),
            current: revision,
        });
    }
    let next = revision.increment();
    if next.digits().len() > INT_DIGIT_LIMIT {
        return Err(CatalogFailure::Validation(WorkflowFailure::Qualification(
            DocumentReason::IntegerTooLong {
                digits: next.digits().len(),
            },
        )));
    }
    Ok(next)
}

fn pin_names(definition: &mut JsonObject, tree: &DependencyTree) {
    if let Some(Json::Array(items)) = definition.get_mut("nodes") {
        for node in items.iter_mut().filter_map(Json::as_object_mut) {
            if node.get("type").and_then(Json::as_str) != Some("workflow") {
                continue;
            }
            if let Some(entry) = node
                .get("workflow_ref")
                .and_then(Json::as_object)
                .and_then(|reference| reference.get("workflow_id"))
                .and_then(Json::as_str)
                .and_then(|id| tree.entry(id))
            {
                node.insert("workflow_name", Json::text(&entry.name));
            }
        }
    }
}
