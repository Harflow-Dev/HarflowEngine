#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_domain::{
    backend::{BackendError, RecordKind},
    ownership::Id,
};
use hf_interfaces::claude_ndjson::{Boundary, Decoder, Limits, Outcome, line_boundary};

const ATTEMPT: &str = "a1b2c3d4e5f60718a1b2c3d4e5f60718";
const RECORDING: &str = include_str!(
    "../../../../HarflowMCP/conformance/claude_fake/recordings/r02_live_single/stdout.jsonl"
);

fn line(value: &str) -> Vec<u8> {
    let mut bytes = value.as_bytes().to_vec();
    bytes.push(b'\n');
    bytes
}

#[test]
fn pinned_live_recording_is_accepted_without_changing_fixture_decoder() {
    let init = line(
        r#"{"type":"system","subtype":"init","session_id":"fake-session","tools":[{"name":"Read"}]}"#,
    );
    let mut fixture = Decoder::new(Id::new(ATTEMPT.into()).unwrap(), Limits::default()).unwrap();
    assert!(matches!(fixture.feed(&init).outcome, Outcome::Fault(_)));

    let mut production =
        Decoder::new_production(Id::new(ATTEMPT.into()).unwrap(), Limits::default()).unwrap();
    let mut records = 0;
    for source in RECORDING.lines() {
        let fed = production.feed(&line(source));
        assert!(fed.consumed > 0, "{source}");
        match fed.outcome {
            Outcome::Event(batch) => records += batch.len(),
            other => panic!("unexpected recording outcome {other:?}: {source}"),
        }
    }
    assert!(records > 1);
    production.finish().unwrap();
}

#[test]
fn production_boundary_decoder_refuses_unknown_nested_and_out_of_range_events_stickily() {
    let accepted = br#"{"type":"stream_event","event":{"type":"content_block_start","index":127}}"#;
    assert_eq!(line_boundary(accepted), Ok(Boundary::BlockStart(127)));
    assert_eq!(
        line_boundary(
            br#"{"type":"stream_event","event":{"type":"content_block_start","index":128}}"#
        ),
        Err(BackendError::Shape)
    );
    assert_eq!(
        line_boundary(br#"{"type":"stream_event","parent_tool_use_id":"tool-1","event":{"type":"message_start"}}"#),
        Err(BackendError::Unsupported)
    );
    assert_eq!(
        line_boundary(br#"{"type":"stream_event","event":{"type":"future_boundary"}}"#),
        Err(BackendError::Unsupported)
    );

    let mut decoder =
        Decoder::new_production(Id::new(ATTEMPT.into()).unwrap(), Limits::default()).unwrap();
    let malformed = line(
        r#"{"type":"stream_event","event":{"type":"future_boundary"},"session_id":"fake-session"}"#,
    );
    assert!(matches!(
        decoder.feed(&malformed).outcome,
        Outcome::Fault(BackendError::Unsupported)
    ));
    assert_eq!(decoder.feed(b"anything").consumed, 0);
}

#[test]
fn production_mode_does_not_turn_unknown_top_level_events_into_metadata() {
    let mut decoder =
        Decoder::new_production(Id::new(ATTEMPT.into()).unwrap(), Limits::default()).unwrap();
    let unknown = line(r#"{"type":"future_effect","session_id":"fake-session"}"#);
    assert!(matches!(
        decoder.feed(&unknown).outcome,
        Outcome::Fault(BackendError::Unsupported)
    ));
    assert_eq!(decoder.feed(&unknown).consumed, 0);

    let mut current =
        Decoder::new_production(Id::new(ATTEMPT.into()).unwrap(), Limits::default()).unwrap();
    let terminal = line(
        r#"{"type":"result","session_id":"fake-session","subtype":"success","is_error":false}"#,
    );
    let Outcome::Event(records) = current.feed(&terminal).outcome else {
        panic!()
    };
    assert_eq!(records[0].kind, RecordKind::Terminal { failed: false });
}
