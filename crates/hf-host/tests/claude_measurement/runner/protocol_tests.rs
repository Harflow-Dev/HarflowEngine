use super::*;

const NONCE: &str = "protocoltestnonce1234";

fn text() -> String {
    format!(
        r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"HARFLOW_FIXTURE_OK:{NONCE}"}}]}}}}"#
    )
}

fn success() -> String {
    format!(
        r#"{{"type":"result","subtype":"success","is_error":false,"result":"HARFLOW_FIXTURE_OK:{NONCE}"}}"#
    )
}

fn stream(events: &[String]) -> Vec<u8> {
    events.join("\n").into_bytes()
}

#[test]
fn protocol_requires_text_before_one_terminal_success() {
    assert_eq!(
        protocol_witnesses(&stream(&[text(), success()]), NONCE),
        Ok(())
    );
    for invalid in [
        stream(&[success()]),
        stream(&[text(), success().replace(NONCE, "wrongnonce-123456")]),
        stream(&[text(), text(), success()]),
        stream(&[text(), success(), success()]),
        stream(&[text(), success(), text()]),
        stream(&[
            text(),
            r#"{"type":"result","subtype":"error","is_error":true}"#.into(),
        ]),
    ] {
        assert!(protocol_witnesses(&invalid, NONCE).is_err());
    }
}

#[test]
fn tool_payload_cannot_complete_protocol() {
    let tool = format!(
        r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"tool-1","name":"lookup","input":{{"nested":{{"value":"HARFLOW_FIXTURE_OK:{NONCE}"}}}}}}]}}}}"#
    );
    assert!(protocol_witnesses(&stream(&[tool, success()]), NONCE).is_err());
}

#[test]
fn capture_completeness_requires_both_eofs_without_overflow_or_read_errors() {
    let complete = result(true, true, false, false, None, None);
    assert!(complete.capture_is_complete());
    assert!(!result(false, true, false, false, None, None).capture_is_complete());
    assert!(!result(true, false, false, false, None, None).capture_is_complete());
    assert!(!result(true, true, true, false, None, None).capture_is_complete());
    assert!(!result(true, true, false, true, None, None).capture_is_complete());
    assert!(!result(true, true, false, false, Some("read failed"), None).capture_is_complete());
    assert!(!result(true, true, false, false, None, Some("read failed")).capture_is_complete());
}

fn result(
    stdout_eof: bool,
    stderr_eof: bool,
    stdout_overflow: bool,
    stderr_overflow: bool,
    stdout_error: Option<&str>,
    stderr_error: Option<&str>,
) -> RunResult {
    RunResult {
        exit: ProcessExit::Code(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
        identity: "synthetic test identity".into(),
        stdout_eof,
        stderr_eof,
        stdout_overflow,
        stderr_overflow,
        stdout_error: stdout_error.map(str::to_owned),
        stderr_error: stderr_error.map(str::to_owned),
    }
}
