#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_domain::ownership::Id;
use hf_interfaces::claude_stream::ClaudeStreamDecoder;

fn decoder() -> ClaudeStreamDecoder {
    ClaudeStreamDecoder::new(Id::new("a1b2c3d4e5f60718a1b2c3d4e5f60718".into()).unwrap())
}

fn push_line(decoder: &mut ClaudeStreamDecoder, line: &str) {
    let mut bytes = line.as_bytes().to_vec();
    bytes.push(b'\n');
    for chunk in bytes.chunks(4096) {
        decoder.push(chunk);
    }
}

#[test]
fn incremental_utf8_and_coalesced_lines_preserve_records() {
    let mut decoder = decoder();
    let init = br#"{"type":"system","subtype":"init","session_id":"cli-1"}"#;
    decoder.push(init);
    decoder.push(b"\n{\"type\":\"assistant\",\"session_id\":\"cli-1\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"caf\xc3");
    decoder.push(b"\xa9\"}]}}\n");
    let trace = decoder.finish();
    assert_eq!(trace.records.len(), 2);
    assert!(trace.fault.is_none());
    assert!(trace.ended);
}

#[test]
fn valid_prefix_and_sticky_failure_survive_malformed_suffix() {
    let mut decoder = decoder();
    push_line(
        &mut decoder,
        r#"{"type":"system","subtype":"init","session_id":"cli-1"}"#,
    );
    push_line(
        &mut decoder,
        r#"{"type":"result","session_id":"cli-1","subtype":"error_during_execution","is_error":true}"#,
    );
    push_line(
        &mut decoder,
        r#"{"type":"assistant","session_id":"cli-1","message":{}}"#,
    );
    let trace = decoder.finish();
    assert_eq!(trace.records.len(), 2);
    assert!(trace.fault.is_some());
    assert!(trace.ended);
}

#[test]
fn accepted_prefix_inside_overflowing_final_chunk_keeps_failure_record() {
    let mut decoder = decoder();
    for _ in 0..80 {
        push_line(
            &mut decoder,
            &format!(
                r#"{{"type":"assistant","session_id":"cli-1","message":{{"content":[{{"type":"text","text":"{}"}}]}}}}"#,
                "x".repeat(700)
            ),
        );
    }
    let mut final_chunk = br#"{"type":"result","session_id":"cli-1","subtype":"error_during_execution","is_error":true}"#.to_vec();
    final_chunk.push(b'\n');
    final_chunk.resize(4096, b' ');
    decoder.push(&final_chunk);
    let trace = decoder.finish();
    assert!(trace.records.iter().any(|record| matches!(
        record.kind,
        hf_domain::backend::RecordKind::Terminal { failed: true }
    )));
    assert!(trace.fault.is_some());
}

#[test]
fn duplicate_keys_invalid_utf8_and_unterminated_lines_fail_closed() {
    let mut duplicate = decoder();
    duplicate.push(br#"{"type":"system","type":"result","subtype":"init","session_id":"x"}"#);
    duplicate.push(b"\n");
    assert!(duplicate.finish().fault.is_some());

    let mut invalid_utf8 = decoder();
    invalid_utf8.push(b"{\xff}\n");
    assert!(invalid_utf8.finish().fault.is_some());

    let mut truncated = decoder();
    truncated.push(br#"{"type":"system","subtype":"init","session_id":"x"}"#);
    assert!(truncated.finish().fault.is_some());
}

#[test]
fn line_chunk_stream_and_record_budgets_are_enforced() {
    let mut chunk = decoder();
    chunk.push(&vec![b'x'; 4097]);
    assert!(chunk.finish().fault.is_some());

    let mut line = decoder();
    for _ in 0..6 {
        line.push(&vec![b'x'; 4096]);
    }
    line.push(&[b'x'; 1]);
    assert!(line.finish().fault.is_some());

    let mut stream = decoder();
    for _ in 0..120 {
        push_line(
            &mut stream,
            &format!(
                r#"{{"type":"assistant","session_id":"cli-1","message":{{"content":[{{"type":"text","text":"{}"}}]}}}}"#,
                "x".repeat(700)
            ),
        );
    }
    assert!(stream.finish().fault.is_some());

    let mut records = decoder();
    for _ in 0..65 {
        push_line(
            &mut records,
            r#"{"type":"assistant","session_id":"cli-1","message":{"content":[{"type":"text","text":"x"},{"type":"text","text":"y"}]}}"#,
        );
    }
    assert!(records.finish().fault.is_some());
}

#[test]
fn excessive_depth_and_unknown_activity_are_not_normalized() {
    let mut deep = decoder();
    deep.push(
        format!(
            "{{\"type\":\"system\",\"subtype\":\"status\",\"session_id\":\"x\",\"extra\":{}}}\n",
            "[".repeat(70) + "0" + &"]".repeat(70)
        )
        .as_bytes(),
    );
    assert!(deep.finish().fault.is_some());

    let mut unknown = decoder();
    push_line(&mut unknown, r#"{"type":"future_effect","session_id":"x"}"#);
    assert!(unknown.finish().fault.is_some());
}

#[test]
fn event_roles_cannot_swap_tool_use_and_tool_result_shapes() {
    let mut user_use = decoder();
    push_line(
        &mut user_use,
        r#"{"type":"system","subtype":"init","session_id":"s"}"#,
    );
    push_line(
        &mut user_use,
        r#"{"type":"user","session_id":"s","message":{"content":[{"type":"tool_use","id":"t"}]}}"#,
    );
    assert!(user_use.finish().fault.is_some());

    let mut assistant_result = decoder();
    push_line(
        &mut assistant_result,
        r#"{"type":"system","subtype":"init","session_id":"s"}"#,
    );
    push_line(
        &mut assistant_result,
        r#"{"type":"assistant","session_id":"s","message":{"content":[{"type":"tool_result","tool_use_id":"t"}]}}"#,
    );
    assert!(assistant_result.finish().fault.is_some());
}

#[test]
fn bounded_identifiers_and_postterminal_activity_refuse() {
    let mut session = decoder();
    push_line(
        &mut session,
        &format!(
            r#"{{"type":"system","subtype":"init","session_id":"{}"}}"#,
            "s".repeat(257)
        ),
    );
    assert!(session.finish().fault.is_some());

    let mut parent = decoder();
    push_line(
        &mut parent,
        r#"{"type":"system","subtype":"init","session_id":"s"}"#,
    );
    push_line(
        &mut parent,
        &format!(
            r#"{{"type":"assistant","session_id":"s","parent_tool_use_id":"{}","message":{{"content":[{{"type":"text","text":"child"}}]}}}}"#,
            "p".repeat(257)
        ),
    );
    assert!(parent.finish().fault.is_some());
}
