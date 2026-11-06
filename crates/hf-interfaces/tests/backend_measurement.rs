#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_interfaces::backend_measurement::{
    ClaudeEvent, FixtureCodecError, anthropic_text_response, decode_anthropic_request,
    decode_claude_event,
};

const NONCE: &str = "a1b2c3d4e5f60718";
const PROMPT: &str =
    "Harflow fixture nonce a1b2c3d4e5f60718. Reply with HARFLOW_FIXTURE_OK:a1b2c3d4e5f60718.";

fn request(content: &str) -> Vec<u8> {
    format!(r#"{{"model":"claude-sonnet","messages":[{{"role":"user","content":"{content}"}}]}}"#)
        .into_bytes()
}

#[test]
fn request_requires_the_exact_nonce_bound_user_message() {
    let decoded = decode_anthropic_request(&request(PROMPT), NONCE).unwrap();
    assert_eq!(decoded.model, "claude-sonnet");
    assert_eq!(
        decode_anthropic_request(&request("ordinary request"), NONCE),
        Err(FixtureCodecError::Nonce)
    );
    assert_eq!(
        decode_anthropic_request(&request(PROMPT), "short"),
        Err(FixtureCodecError::Nonce)
    );
}

#[test]
fn request_rejects_tools_duplicate_keys_and_oversize_input() {
    let tools = format!(
        r#"{{"model":"m","messages":[{{"role":"user","content":"{PROMPT}"}}],"tools":[{{"name":"x"}}]}}"#
    );
    assert_eq!(
        decode_anthropic_request(tools.as_bytes(), NONCE),
        Err(FixtureCodecError::Tools)
    );
    let duplicate = format!(
        r#"{{"model":"m","model":"other","messages":[{{"role":"user","content":"{PROMPT}"}}]}}"#
    );
    assert_eq!(
        decode_anthropic_request(duplicate.as_bytes(), NONCE),
        Err(FixtureCodecError::Shape)
    );
    assert_eq!(
        decode_anthropic_request(&vec![b' '; 24_577], NONCE),
        Err(FixtureCodecError::Budget)
    );
}

#[test]
fn request_rejects_nonempty_advertised_tools_but_accepts_empty_list() {
    let empty = format!(
        r#"{{"model":"m","messages":[{{"role":"user","content":"{PROMPT}"}}],"tools":[]}}"#
    );
    assert!(decode_anthropic_request(empty.as_bytes(), NONCE).is_ok());
    let advertised = format!(
        r#"{{"model":"m","messages":[{{"role":"user","content":"{PROMPT}"}}],"tools":[{{"name":"lookup","input_schema":{{"type":"object"}}}}]}}"#
    );
    assert_eq!(
        decode_anthropic_request(advertised.as_bytes(), NONCE),
        Err(FixtureCodecError::Tools)
    );
}

#[test]
fn claude_tool_content_is_not_a_text_or_terminal_witness() {
    for event in [
        format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"t1","name":"lookup","input":{{"q":"{NONCE}"}}}}]}}}}"#
        ),
        format!(
            r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"t1","content":"HARFLOW_FIXTURE_OK:{NONCE}"}}]}}}}"#
        ),
        format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"HARFLOW_FIXTURE_OK:{NONCE}"}},{{"type":"tool_use","id":"t1","name":"lookup","input":{{"nested":{{"x":1}}}}}}]}}}}"#
        ),
    ] {
        assert_eq!(
            decode_claude_event(event.as_bytes(), NONCE),
            Err(FixtureCodecError::Tools)
        );
    }
}

#[test]
fn mcp_tool_events_mixed_with_marker_are_rejected_as_tools() {
    for event in [
        format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"HARFLOW_FIXTURE_OK:{NONCE}"}},{{"type":"mcp_tool_use","id":"mcp1","name":"lookup","input":{{"nested":{{"nonce":"{NONCE}"}}}}}}]}}}}"#
        ),
        format!(
            r#"{{"type":"user","message":{{"content":[{{"type":"mcp_tool_result","tool_use_id":"mcp1","content":"HARFLOW_FIXTURE_OK:{NONCE}"}}]}}}}"#
        ),
    ] {
        assert_eq!(
            decode_claude_event(event.as_bytes(), NONCE),
            Err(FixtureCodecError::Tools)
        );
    }

    for kind in [
        "web_search_tool_result",
        "code_execution_tool_result",
        "bash_code_execution_tool_result",
        "tool_reference",
    ] {
        let event = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"HARFLOW_FIXTURE_OK:{NONCE}"}},{{"type":"{kind}","input":{{"nested":{{"value":1}}}}}}]}}}}"#
        );
        assert_eq!(
            decode_claude_event(event.as_bytes(), NONCE),
            Err(FixtureCodecError::Tools),
            "tool-family event {kind} must be refused"
        );
    }
}

#[test]
fn plaintext_mentioning_tool_use_does_not_trigger_tool_refusal() {
    let event = format!(
        r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"The literal words tool_use web_search_tool_result code_execution_tool_result bash_code_execution_tool_result tool_reference are documentation. HARFLOW_FIXTURE_OK:{NONCE}"}}]}}}}"#
    );
    assert_eq!(
        decode_claude_event(event.as_bytes(), NONCE),
        Ok(ClaudeEvent::Other)
    );
}

#[test]
fn response_is_fixed_text_sse_and_event_terminal_is_only_a_protocol_witness() {
    let response = anthropic_text_response("claude-sonnet", NONCE).unwrap();
    let text = String::from_utf8(response).unwrap();
    assert!(text.contains(&format!("HARFLOW_FIXTURE_OK:{NONCE}")));
    assert!(!text.contains("tool_use"));
    assert_eq!(decode_claude_event(format!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"HARFLOW_FIXTURE_OK:{NONCE}"}}"#).as_bytes(), NONCE), Ok(ClaudeEvent::TerminalSuccess));
    assert_eq!(decode_claude_event(br#"{"type":"result","subtype":"success","is_error":false,"result":"HARFLOW_FIXTURE_OK:wrongnonce12345"}"#, NONCE), Ok(ClaudeEvent::TerminalFailure));
    assert_eq!(
        decode_claude_event(b"{}", NONCE),
        Err(FixtureCodecError::Shape)
    );
}
