//! JSON documents: sorted keys, compact, envelope stamped after the payload.
use hf_domain::ctl::{
    identity::{
        CAPABILITIES, CTL_FORMAT, EVENT_FORMAT, HostIdentity, PRODUCT, PRODUCT_VERSION,
        SETUP_FORMAT, WIRE_VERSION,
    },
    inspect::{Scalar, StateInspection},
    state_root::StateRoot,
};
use serde_json::{Map, Value, json};
/// Reserved identity fields are written last so a payload cannot override them.
pub fn stamp(format: &str, payload: Value) -> Value {
    let mut map = match payload {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    map.insert("product".into(), json!(PRODUCT));
    map.insert("format".into(), json!(format));
    map.insert("v".into(), json!(WIRE_VERSION));
    map.insert("product_version".into(), json!(PRODUCT_VERSION));
    Value::Object(map)
}
pub fn line(value: &Value) -> String {
    let mut text = value.to_string();
    text.push('\n');
    text
}
pub fn pretty(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    text
}
pub fn usage_error(format: &str, message: &str) -> Value {
    stamp(
        format,
        json!({"error": {"code": "usage", "message": message}}),
    )
}
pub fn version_info(root: &StateRoot, host: &HostIdentity) -> Value {
    let contract = |format: &str| json!({"format": format, "v": WIRE_VERSION});
    stamp(
        hf_domain::ctl::identity::VERSION_INFO_FORMAT,
        json!({
            "component": "engine",
            "contracts": {
                "ctl": contract(CTL_FORMAT),
                "setup": contract(SETUP_FORMAT),
                "events": contract(EVENT_FORMAT),
            },
            "capabilities": CAPABILITIES,
            "os": host.os,
            "architecture": host.architecture,
            "state_root": root.as_str(),
        }),
    )
}
/// The raw `state-inspect` result, without an envelope.
pub fn inspection(inspection: &StateInspection) -> Value {
    let mut result = json!({
        "state_root": inspection.state_root,
        "engine_version": PRODUCT_VERSION,
        "recovery_supported": false,
        "cooperative_ownership_only": true,
        "status": inspection.status.as_str(),
        "reason": inspection.reason.as_str(),
        "message": inspection.reason.message(inspection.status),
    });
    if !inspection.observed_store.is_empty() {
        let mut observed = Map::new();
        for field in &inspection.observed_store {
            let value = match &field.value {
                Scalar::Int(number) => json!(number),
                Scalar::Text(text) => json!(text),
            };
            observed.insert(field.key.to_string(), value);
        }
        if let Value::Object(map) = &mut result {
            map.insert("observed_store".into(), Value::Object(observed));
        }
    }
    result
}
