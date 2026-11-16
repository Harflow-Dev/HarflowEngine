//! Bounded, occurrence-based diagnostics, independent of resolver pin memoization.
use super::identity::workflow_identity;
use super::ports::DefinitionCodec;
use super::validate::Validator;
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::refusal::{MissingReference, ReferenceSegment};
use hf_domain::workflow::tree::text;
use hf_domain::workflow::vocab::{MAX_JSON_DEPTH, MAX_NESTING_DEPTH};
use std::collections::HashMap;

const MAX_REPORTS: usize = 256;
const MAX_VISITS: usize = 10000;
const MAX_BYTES: usize = 131072;
const MAX_ID: usize = 1024;
const MAX_DISPLAY: usize = 200;

struct Frame<'a> {
    definition: &'a JsonObject,
    id: String,
    cursor: usize,
    path: Vec<ReferenceSegment>,
}

fn segment_json(segment: &ReferenceSegment) -> JsonObject {
    let mut object = JsonObject::new();
    object.insert("workflow_id", Json::text(&segment.workflow_id));
    object.insert("workflow_name", Json::text(&segment.workflow_name));
    object.insert("node_id", Json::text(&segment.node_id));
    object.insert("node_title", Json::text(&segment.node_title));
    object
}

fn report_json(report: &MissingReference) -> Json {
    let mut object = segment_json(&report.segment);
    object.insert("target_workflow_id", Json::text(&report.target_workflow_id));
    object.insert(
        "dependency_path",
        Json::Array(
            report
                .dependency_path
                .iter()
                .map(|segment| Json::Object(segment_json(segment)))
                .collect(),
        ),
    );
    Json::Object(object)
}

fn admissible(
    validator: &Validator<'_>,
    codec: &dyn DefinitionCodec,
    definition: &JsonObject,
    index: &HashMap<String, JsonObject>,
) -> bool {
    if definition.nests_beyond(MAX_JSON_DEPTH) {
        return false;
    }
    if validator
        .validate(&Json::Object(definition.clone()))
        .is_err()
    {
        return false;
    }
    let id = workflow_identity(codec, definition);
    index.get(&id).is_none_or(|indexed| {
        !indexed.nests_beyond(MAX_JSON_DEPTH)
            && codec.definition_sha256(indexed) == codec.definition_sha256(definition)
    })
}

fn make_segment(frame: &Frame<'_>, node: &JsonObject, node_id: &str) -> ReferenceSegment {
    let title = node
        .get("title")
        .and_then(Json::as_str)
        .filter(|title| !title.is_empty())
        .unwrap_or(node_id);
    ReferenceSegment {
        workflow_id: frame.id.clone(),
        workflow_name: text(frame.definition, "name")
            .chars()
            .take(MAX_DISPLAY)
            .collect(),
        node_id: node_id.to_string(),
        node_title: title.chars().take(MAX_DISPLAY).collect(),
    }
}

/// Repeated references retain each calling path; only per-definition validity is cached.
/// Over-depth inputs are not admissible and are checked before recursive clone or hash.
pub fn missing_references(
    validator: &Validator<'_>,
    codec: &dyn DefinitionCodec,
    root: &JsonObject,
    index: &HashMap<String, JsonObject>,
) -> (Vec<MissingReference>, bool) {
    let mut reports = Vec::new();
    if !admissible(validator, codec, root, index) {
        return (reports, false);
    }
    let mut valid = HashMap::new();
    let mut stack = Vec::from([Frame {
        definition: root,
        id: workflow_identity(codec, root),
        cursor: 0,
        path: Vec::new(),
    }]);
    let mut visits = 0;
    let mut bytes = 2;
    while let Some(frame) = stack.last_mut() {
        let Some(node) = frame
            .definition
            .get("nodes")
            .and_then(Json::as_array)
            .and_then(|items| items.get(frame.cursor))
            .and_then(Json::as_object)
        else {
            stack.pop();
            continue;
        };
        frame.cursor += 1;
        visits += 1;
        if visits > MAX_VISITS {
            return (reports, true);
        }
        if text(node, "type") != "workflow" {
            continue;
        }
        let Some((target, node_id)) = reference_ids(&frame.id, node) else {
            return (reports, true);
        };
        let segment = make_segment(frame, node, node_id);
        let mut path = frame.path.clone();
        path.push(segment.clone());
        if let Some(child) = index.get(target) {
            let cyclic = stack.iter().any(|ancestor| ancestor.id == target);
            if cyclic || stack.len() >= MAX_NESTING_DEPTH {
                continue;
            }
            if !*valid
                .entry(target.to_string())
                .or_insert_with(|| admissible(validator, codec, child, index))
            {
                continue;
            }
            stack.push(Frame {
                definition: child,
                id: target.to_string(),
                cursor: 0,
                path,
            });
        } else {
            let report = MissingReference {
                segment,
                target_workflow_id: target.to_string(),
                dependency_path: path,
            };
            if !append_report(codec, &mut reports, &mut bytes, report) {
                return (reports, true);
            }
        }
    }
    (reports, false)
}

fn reference_ids<'a>(id: &str, node: &'a JsonObject) -> Option<(&'a str, &'a str)> {
    let target = node
        .get("workflow_ref")?
        .as_object()?
        .get("workflow_id")?
        .as_str()?;
    let node_id = node.get("id")?.as_str()?;
    if [id, node_id, target]
        .iter()
        .any(|value| value.chars().count() > MAX_ID)
    {
        return None;
    }
    Some((target, node_id))
}

fn append_report(
    codec: &dyn DefinitionCodec,
    reports: &mut Vec<MissingReference>,
    bytes: &mut usize,
    report: MissingReference,
) -> bool {
    if reports.len() >= MAX_REPORTS {
        return false;
    }
    let size = codec.ascii_dumps_len(&report_json(&report)) + 2;
    if *bytes + size > MAX_BYTES {
        return false;
    }
    *bytes += size;
    reports.push(report);
    true
}
