#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_domain::{
    backend::{BackendError, RecordKind},
    ownership::Id,
};
use hf_interfaces::claude_ndjson::{Decoder, Limits, Outcome};

#[path = "claude_ndjson/boundaries.rs"]
mod boundaries;

const ATTEMPT: &str = "a1b2c3d4e5f60718a1b2c3d4e5f60718";
const INIT: &[u8] = br#"{"type":"system","subtype":"init","session_id":"cli-1"}"#;

fn decoder(limits: Limits) -> Decoder {
    Decoder::new(Id::new(ATTEMPT.into()).unwrap(), limits).unwrap()
}
fn line(payload: &[u8]) -> Vec<u8> {
    let mut result = payload.to_vec();
    result.push(b'\n');
    result
}
fn event(outcome: Outcome) -> Vec<hf_domain::backend::BackendRecord> {
    match outcome {
        Outcome::Event(records) => records,
        other => panic!("expected event, got {other:?}"),
    }
}

#[test]
fn every_utf8_chunk_boundary_and_coalesced_remainder_are_exact() {
    let text = r#"{"type":"assistant","session_id":"s","message":{"content":[{"type":"text","text":"café 🐈"}]}}"#.as_bytes();
    let encoded = line(text);
    for split in 0..=encoded.len() {
        let mut d = decoder(Limits::default());
        let prefix = d.feed(&encoded[..split]);
        assert_eq!(prefix.consumed, split);
        if matches!(prefix.outcome, Outcome::Event(_)) {
            assert_eq!(split, encoded.len());
        } else {
            let result = d.feed(&encoded[split..]);
            assert_eq!(result.consumed, encoded.len() - split);
            assert!(matches!(result.outcome, Outcome::Event(_)), "split {split}");
        }
    }

    let mut d = decoder(Limits::default());
    let first = line(INIT);
    let second = line(INIT);
    let mut joined = first.clone();
    joined.extend_from_slice(&second);
    let got = d.feed(&joined);
    assert_eq!(got.consumed, first.len());
    assert_eq!(event(got.outcome).len(), 1);
    let got = d.feed(&joined[got.consumed..]);
    assert_eq!(got.consumed, second.len());
    assert_eq!(event(got.outcome).len(), 1);
}

#[test]
fn input_consumption_is_capped_and_pending_storage_is_only_a_partial_line() {
    let mut d = decoder(Limits::default());
    let mut stream = Vec::new();
    for _ in 0..1200 {
        stream.extend(line(INIT));
    }
    assert!(stream.len() > 65_536);
    let mut offset = 0;
    let mut emitted = 0;
    while offset < stream.len() {
        let fed = d.feed(&stream[offset..]);
        assert!(fed.consumed <= 4096);
        offset += fed.consumed;
        assert!(d.pending_bytes() <= Limits::default().max_line_bytes);
        match fed.outcome {
            Outcome::Event(records) => {
                emitted += records.len();
            }
            Outcome::NeedMore => assert_eq!(offset, stream.len()),
            other => panic!("unexpected outcome {other:?}"),
        }
    }
    assert_eq!(emitted, 1200);

    let mut many = decoder(Limits::default());
    let record = line(br#"{"type":"assistant","session_id":"s","message":{"content":[{"type":"text","text":"ok"}]}}"#);
    for _ in 0..160 {
        let mut cursor = 0;
        while cursor < record.len() {
            let fed = many.feed(&record[cursor..]);
            cursor += fed.consumed;
            assert!(matches!(fed.outcome, Outcome::NeedMore | Outcome::Event(_)));
        }
        assert_eq!(many.pending_bytes(), 0);
    }
}

#[test]
fn injected_event_identifier_depth_collection_and_record_limits_apply() {
    let mut id_limited = decoder(Limits {
        max_identifier_bytes: 3,
        ..Limits::default()
    });
    assert_eq!(
        id_limited.feed(&line(INIT)).outcome,
        Outcome::Fault(BackendError::Shape)
    );

    let mut event_limited = decoder(Limits {
        max_event_bytes: 1,
        ..Limits::default()
    });
    assert_eq!(
        event_limited.feed(&line(INIT)).outcome,
        Outcome::Fault(BackendError::Budget)
    );

    let mut depth_limited = decoder(Limits {
        max_depth: 1,
        ..Limits::default()
    });
    let nested = line(br#"{"type":"assistant","session_id":"s","message":{"content":[{"type":"text","text":"x"}]}}"#);
    assert_eq!(
        depth_limited.feed(&nested).outcome,
        Outcome::Fault(BackendError::Budget)
    );

    let multi = line(br#"{"type":"assistant","session_id":"s","message":{"content":[{"type":"text","text":"x"},{"type":"text","text":"y"}]}}"#);
    let mut collection_limited = decoder(Limits {
        max_collection_members: 1,
        ..Limits::default()
    });
    assert_eq!(
        collection_limited.feed(&multi).outcome,
        Outcome::Fault(BackendError::Budget)
    );

    let mut records_limited = decoder(Limits {
        max_records: 1,
        ..Limits::default()
    });
    assert_eq!(
        records_limited.feed(&multi).outcome,
        Outcome::Fault(BackendError::Budget)
    );

    let mut key_limited = decoder(Limits {
        max_key_bytes: 3,
        ..Limits::default()
    });
    assert_eq!(
        key_limited.feed(&line(INIT)).outcome,
        Outcome::Fault(BackendError::Budget)
    );
}

#[test]
fn malformed_duplicate_invalid_utf8_truncated_and_over_budget_are_sticky() {
    for bad in [
        line(br#"{"type":"system","type":"result","subtype":"init","session_id":"s"}"#),
        line(br#"{"type":"system","ty\u0070e":"result","subtype":"init","session_id":"s"}"#),
        line(b"{\xff}"),
        line(br#"{"type":"system","subtype":"init","session_id":"s"} trailing"#),
        line(br#"{"type":"future_effect","session_id":"s"}"#),
    ] {
        let mut d = decoder(Limits::default());
        let first = d.feed(&bad);
        assert!(matches!(first.outcome, Outcome::Fault(_)));
        assert_eq!(d.pending_bytes(), 0);
        assert_eq!(d.feed(INIT).consumed, 0);
        assert!(matches!(d.feed(INIT).outcome, Outcome::Fault(_)));
    }
    let mut truncated = decoder(Limits::default());
    truncated.feed(INIT);
    assert_eq!(truncated.finish(), Err(BackendError::Truncated));
    assert_eq!(truncated.finish(), Err(BackendError::Truncated));

    let mut d = decoder(Limits {
        max_line_bytes: 8,
        ..Limits::default()
    });
    let fed = d.feed(b"123456789more\n");
    assert_eq!(fed.consumed, 9);
    assert_eq!(fed.outcome, Outcome::Fault(BackendError::Budget));
    assert_eq!(d.pending_bytes(), 0);
}

#[test]
fn a_returned_valid_prefix_survives_a_later_sticky_fault() {
    let mut d = decoder(Limits::default());
    let accepted = d.feed(&line(INIT));
    assert_eq!(event(accepted.outcome).len(), 1);
    let malformed = line(br#"{"type":"system","subtype":"init","session_id":}"#);
    let fault = d.feed(&malformed);
    assert_eq!(fault.outcome, Outcome::Fault(BackendError::Shape));
    assert_eq!(
        d.feed(&line(INIT)).outcome,
        Outcome::Fault(BackendError::Shape)
    );
}

#[test]
fn finish_is_idempotent_and_terminal_or_postterminal_are_only_observations() {
    let terminal =
        line(br#"{"type":"result","session_id":"s","subtype":"success","is_error":false}"#);
    let mut d = decoder(Limits::default());
    let records = event(d.feed(&terminal).outcome);
    assert!(matches!(
        records[0].kind,
        RecordKind::Terminal { failed: false }
    ));
    assert_eq!(d.finish(), Ok(()));
    assert_eq!(d.finish(), Ok(()));
    assert_eq!(d.feed(INIT).consumed, 0);
    assert_eq!(d.feed(INIT).outcome, Outcome::Closed);

    let mut clean = decoder(Limits::default());
    assert_eq!(clean.finish(), Ok(()));
    assert_eq!(clean.feed(INIT).outcome, Outcome::Closed);

    let mut after_terminal = decoder(Limits::default());
    assert!(matches!(
        after_terminal.feed(&terminal).outcome,
        Outcome::Event(_)
    ));
    let late = line(br#"{"type":"assistant","session_id":"s","message":{"content":[{"type":"text","text":"late"}]}}"#);
    assert!(matches!(
        after_terminal.feed(&late).outcome,
        Outcome::Event(_)
    ));
}

#[test]
fn legacy_offline_decoder_keeps_its_total_stream_budget() {
    use hf_interfaces::claude_stream::ClaudeStreamDecoder;
    let mut old = ClaudeStreamDecoder::new(Id::new(ATTEMPT.into()).unwrap());
    let line = line(INIT);
    for _ in 0..1200 {
        old.push(&line);
    }
    assert_eq!(old.finish().fault, Some(BackendError::Budget));
}
