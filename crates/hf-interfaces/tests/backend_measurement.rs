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
