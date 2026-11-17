//! Production projection: exact current unrestricted Claude enforcement disclosure.
//! The deprecated publication-intent alias is retained from the canonical Python wire owner.
use hf_application::{task_lifecycle::InitialProjectionFacts, task_runtime::InvalidIntent};
use serde_json::{Value, json};
pub(super) fn enforcement() -> Value {
    json!({"extra_writable_paths":[],"extra_path_certification":null,"freedom":"unrestricted","mechanism":"claude --permission-mode bypassPermissions, no deny patterns","os_enforced":false,"writes_confined":false,"writable_roots":[],"commit_push_blocked":false,"direct_commit_commands_denied":false,"local_commit":"allowed","publish_attempts_allowed":true,"publish_attempts_allowed_by_polybridge":true,"network_access":"not_controlled","caveats":["no OS sandbox: the agent can read and write outside repo_path, which is only its working directory","bypassPermissions removes that gate, but inherited ask and deny rules can still restrict actions; it is not an OS sandbox","compatibility break: the deny patterns used to apply at unrestricted too, so ordinary git commit/push were refused even there; they are now dropped, same as at publish. Measured (Gate D): bypassPermissions with no --disallowedTools, against a scratch repo with a local bare remote — git commit and git push both succeeded, permission_denials was empty, and the commit landed in both the working repo and the remote"]})
}
pub fn spawn_projection(
    f: &InitialProjectionFacts<'_>,
) -> Result<(Vec<u8>, Vec<u8>), InvalidIntent> {
    if f.status != "running" {
        return Err(InvalidIntent);
    }
    let value = json!({"task_id":f.task.as_str(),"backend":f.backend,"repo_path":f.repo_path,
        "session_id":f.session_id,"status":f.status,"started_at":f.started_at,"title":f.title,
        "model":f.model,"continuation_of_task_id":f.continuation_of.map(|v|v.as_str()),
        "chain_root_task_id":f.chain_root,"exit_code":null,"finished_at":null,"output_drained":false});
    with_enforcement(value)
}
pub(super) fn with_enforcement(mut value: Value) -> Result<(Vec<u8>, Vec<u8>), InvalidIntent> {
    let object = value.as_object_mut().ok_or(InvalidIntent)?;
    let terminal = object.get("output_drained").and_then(Value::as_bool) == Some(true);
    // Absence of a backend/runtime observation is represented as null/unknown, not zero.
    for defaults in baseline_groups() {
        for (key, value) in defaults.as_object().ok_or(InvalidIntent)? {
            object.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    unavailable_summaries(object);
    object.insert(
        "output_capture".into(),
        Value::from(if terminal {
            "observed_terminal"
        } else {
            "unknown"
        }),
    );
    object.insert(
        "input_channel".into(),
        Value::from(if terminal { "closed" } else { "unknown" }),
    );
    object.insert("supervision".into(),json!({"capture_state":if terminal {"observed_terminal"}else{"unknown"},"last_captured_output_at":null,"last_captured_output_source":null,"live_input_state":if terminal {"closed"}else{"unknown"},"owner_state":"undecidable","process_state":"undecidable"}));
    object.insert("freedom".into(), Value::from("unrestricted"));
    object.insert("enforcement".into(), enforcement());
    let bytes = serde_json::to_vec(&value).map_err(|_| InvalidIntent)?;
    if bytes.len() > 16384 {
        return Err(InvalidIntent);
    }
    Ok((bytes.clone(), bytes))
}

fn baseline_groups() -> [Value; 4] {
    [
        json!({"duration_seconds":null,"parent_task_id":null,"auto_continue":0,"spawned_by":null,
            "root_task_id":null,"depth":0,"max_depth":null,"group":null,"lineage_detected":null,
            "live_input":true,"background_idle_seconds":null,"summary":null,"failure_diagnostic":null}),
        json!({"is_error":null,"total_cost_usd":null,"num_turns":null,"permission_denials":[],
            "denial_categories":[],"last_output_tail":[],"raw_stream_log":null,"reasoning_effort":null,
            "max_turns":null,"mcp_servers":[],"available_tool_count":null,"usage":null,"notices":[]}),
        json!({"owner":null,"owned_by_live_server":null,"base_commit":null,"start_dirty":null,
            "outcome":null,"events_log":null,"resume_command":null,"extra_writable_paths":[],
            "extra_path_certification":null,"allowed_commands":[],"metadata_persisted":true,
            "note":"Runtime observer identity and backend metrics are not asserted by the durable projection.",
            "caller_approvals":false,"caller_write_approvals":false}),
        json!({"dispatch_origin":null,"interaction_owner":"caller","last_captured_output_at":null,
            "messages":[],"pending_messages":[],"phase_slots":[],"queued_followup_count":null,"recovered":true,"responding_server":null,"observation_mode":"unknown",
            "observation_note":"Current responding-process identity is not persisted as historical task authority."}),
    ]
}

fn unavailable_summaries(object: &mut serde_json::Map<String, Value>) {
    // Canonical followups._UNAVAILABLE_VIEW; count_outstanding returns null for this state.
    object.insert(
        "queued_followups".into(),
        json!({"state":"unavailable","items":[],"omitted_final":0}),
    );
    // Canonical artifacts.summary refusal. Its zero is not an observed empty inventory.
    object.insert(
        "artifact_summary".into(),
        json!({"count":0,"state":"unavailable"}),
    );
    // Canonical approvals.summary refusal: uncertainty/error take precedence over zero counters.
    object.insert(
        "approval_summary".into(),
        json!({"pending_count":0,"awaiting_decision_count":0,
        "delivery_uncertain_count":0,"interaction_state":"approval_state_uncertain",
        "requests":[],"uncertainty_requests":[],
        "error":"Approval ledger unavailable; execution must be reconciled"}),
    );
    // Canonical recovery_activity.page when capture loss is not confirmed; no transcript probe.
    object.insert(
        "recovery_activity".into(),
        json!({"source":"backend_transcript","display_only":true,
        "events":[],"next_cursor":null,"has_more":false,"reset":false,
        "notices":["Transcript recovery requires confirmed capture loss"]}),
    );
}
