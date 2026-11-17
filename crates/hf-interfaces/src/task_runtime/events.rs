//! Existing v1 event envelopes projected from a strictly admitted committed backend line.
use hf_application::task_runtime::InvalidIntent;
use hf_domain::task_read::TaskId;
use serde_json::{Value, json};
pub struct NormalizedEvent {
    pub kind: String,
    pub body: Vec<u8>,
    pub truncated: bool,
}
pub fn backend_events(
    line: &[u8],
    task: &TaskId,
    first_seq: u64,
    at: &str,
    offset: u64,
) -> Result<Vec<NormalizedEvent>, InvalidIntent> {
    let source: Value = serde_json::from_slice(line).map_err(|_| InvalidIntent)?;
    let mut fields = Vec::new();
    match source["type"].as_str(){
        Some("system") if source["subtype"]=="init"=>fields.push(json!({"kind":"task_started","backend":"claude","session_id":source["session_id"]})),
        Some("assistant") if source["parent_tool_use_id"].is_null()=>{for (index,item) in source["message"]["content"].as_array().ok_or(InvalidIntent)?.iter().enumerate(){match item["type"].as_str(){
            Some("text")=>{let mut event=json!({"kind":"assistant_text","text":item["text"]});if source["message"]["id"].is_string(){event["message_id"]=source["message"]["id"].clone();event["block_index"]=Value::from(index);}fields.push(event);},
            Some("tool_use")=>fields.push(json!({"kind":"tool_call","call_id":item["id"],"tool":item["name"],"category":"other","input_preview":serde_json::to_string(&item["input"]).map_err(|_|InvalidIntent)?})),_=>()
        }}},
        Some("user")=>{for item in source["message"]["content"].as_array().ok_or(InvalidIntent)?{if item["type"]=="tool_result"{fields.push(json!({"kind":"tool_result","call_id":item["tool_use_id"],"ok":item["is_error"]!=true,"output_tail":if let Some(text)=item["content"].as_str(){text.to_owned()}else{serde_json::to_string(&item["content"]).map_err(|_|InvalidIntent)?}}));}}},
        Some("result")=>fields.push(json!({"kind":"usage","usage":source["usage"],"total_cost_usd":source["total_cost_usd"],"num_turns":source["num_turns"]})),_=>()
    }
    let mut output = Vec::new();
    for (index, mut value) in fields.into_iter().enumerate() {
        let seq = first_seq
            .checked_add(index as u64)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or(InvalidIntent)?;
        let object = value.as_object_mut().ok_or(InvalidIntent)?;
        for (key, value) in [
            ("product", json!("harflow")),
            ("format", json!("harflow.event")),
            ("v", json!(1)),
            (
                "product_version",
                json!(hf_domain::ctl::identity::PRODUCT_VERSION),
            ),
            ("seq", json!(seq)),
            ("observed_at", json!(at)),
            (
                "source_ts",
                source.get("timestamp").cloned().unwrap_or(Value::Null),
            ),
            ("raw_offset", json!(offset)),
            ("task_id", json!(task.as_str())),
        ] {
            object.insert(key.into(), value);
        }
        let kind = value["kind"].as_str().ok_or(InvalidIntent)?.to_owned();
        let (body, truncated) = super::event_bounds::bounded(value)?;
        output.push(NormalizedEvent {
            kind,
            body,
            truncated,
        });
    }
    Ok(output)
}
