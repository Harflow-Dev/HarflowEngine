//! Encoded lengths of the artifact manifest document, sized as the reference writes it:
//! `json.dumps(manifest, ensure_ascii=False, allow_nan=False)` with the default separators.
//! The storage writer measures the serialized manifest from these lengths without encoding
//! JSON itself: the envelope (the document with an empty `items` list), each item, and two
//! bytes (`", "`) between consecutive items.
use crate::ctl::bound::utf8_size;
use hf_domain::ctl::identity::{PRODUCT, PRODUCT_VERSION, WIRE_VERSION};
use serde_json::{Map, Number, Value, json};

/// The manifest document's format name.
pub const MANIFEST_FORMAT: &str = "harflow.task.artifacts";

/// One manifest item as the reference stores it. `mtime_ns` holds both a signed stat value
/// and an unsigned one a fixture records; a value outside both 64-bit ranges is not encodable.
#[derive(Clone, Copy, Debug)]
pub struct ManifestItem<'a> {
    pub task_id: &'a str,
    pub artifact_id: &'a str,
    pub path: &'a str,
    pub kind: &'a str,
    pub note: &'a str,
    pub registered_at: &'a str,
    /// `provenance.method`.
    pub method: &'a str,
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub mtime_ns: i128,
    /// `provenance.tool_use_id` of an observed backend plan.
    pub tool_use_id: Option<&'a str>,
    /// `provenance.plan_directory` and its `plan_directory_identity` device and inode.
    pub plan_directory: Option<(&'a str, u64, u64)>,
    /// The workflow binding a workflow-bound caller's item carries.
    pub workflow: Option<&'a Value>,
}

/// The bytes of the manifest of `task_id` with an empty `items` list.
pub fn envelope_len(task_id: &str) -> usize {
    utf8_size(&json!({
        "product": PRODUCT,
        "format": MANIFEST_FORMAT,
        "v": WIRE_VERSION,
        "product_version": PRODUCT_VERSION,
        "task_id": task_id,
        "items": [],
    }))
}

/// The bytes of one item in the manifest's `items` list; `None` when `mtime_ns` does not fit
/// 64 bits.
pub fn item_len(item: &ManifestItem<'_>) -> Option<usize> {
    item_value(item).map(|value| utf8_size(&value))
}

/// The item document, for sizing and equality only, not wire output: its keys are sorted
/// (serde_json without `preserve_order`), not in the reference's order, which does not change
/// the size. `None` when `mtime_ns` does not fit 64 bits.
pub fn item_value(item: &ManifestItem<'_>) -> Option<Value> {
    let mtime_ns = wide(item.mtime_ns)?;
    let mut provenance = Map::new();
    provenance.insert("method".into(), json!(item.method));
    provenance.insert("task_id".into(), json!(item.task_id));
    if let Some(tool_use_id) = item.tool_use_id {
        provenance.insert("tool_use_id".into(), json!(tool_use_id));
        if let Some((path, device, inode)) = item.plan_directory {
            provenance.insert("plan_directory".into(), json!(path));
            provenance.insert(
                "plan_directory_identity".into(),
                json!({"device": device, "inode": inode}),
            );
        }
    }
    let mut value = json!({
        "artifact_id": item.artifact_id,
        "task_id": item.task_id,
        "path": item.path,
        "kind": item.kind,
        "note": item.note,
        "registered_at": item.registered_at,
        "provenance": provenance,
        "registered_observation": {
            "device": item.device,
            "inode": item.inode,
            "size": item.size,
            "mtime_ns": mtime_ns,
        },
    });
    if let (Some(workflow), Value::Object(map)) = (item.workflow, &mut value) {
        map.insert("workflow".into(), workflow.clone());
    }
    Some(value)
}

/// A 64-bit integer as JSON.
fn wide(number: i128) -> Option<Value> {
    match i64::try_from(number) {
        Ok(value) => Some(Value::Number(Number::from(value))),
        Err(_) => u64::try_from(number)
            .ok()
            .map(|v| Value::Number(Number::from(v))),
    }
}
