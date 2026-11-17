#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::{initial, task_id};
use hf_application::task_runtime::{InvalidIntent, TerminalProjectionFacts};
use hf_domain::ownership::Digest;
use hf_interfaces::task_runtime::{
    BlobChunk, BlobTransfer, CHUNK_BYTES, FRAME_BYTES, decode_chunk, encode_chunk,
    encode_live_message, task_finished, terminal_projection,
};

fn digest() -> Digest {
    Digest::new("a".repeat(64)).unwrap()
}

#[test]
fn live_input_is_one_canonical_jsonl_user_message_and_prompt_is_not_reinterpreted() {
    let prompt = "Review this line\n\"as text\" --dangerously-skip-permissions";
    let encoded = encode_live_message(prompt).unwrap();
    assert_eq!(encoded.iter().filter(|byte| **byte == b'\n').count(), 1);
    assert_eq!(encoded.last(), Some(&b'\n'));
    let decoded: serde_json::Value = serde_json::from_slice(&encoded[..encoded.len() - 1]).unwrap();
    assert_eq!(decoded["type"], "user");
    assert_eq!(decoded["message"]["role"], "user");
    assert_eq!(decoded["message"]["content"][0]["text"], prompt);
    assert_eq!(
        encode_live_message(&"x".repeat(4 * 1024 * 1024 + 1)),
        Err(InvalidIntent)
    );
}

#[test]
fn counted_chunks_preserve_order_and_refuse_replay_or_partial_completion() {
    let bytes = vec![b'p'; CHUNK_BYTES + 7];
    let mut transfer = BlobTransfer::new(bytes.len() as u32, digest(), 4 * 1024 * 1024).unwrap();
    let first = BlobChunk {
        offset: 0,
        total: bytes.len() as u32,
        digest: digest(),
        bytes: bytes[..CHUNK_BYTES].to_vec(),
    };
    let second = BlobChunk {
        offset: CHUNK_BYTES as u32,
        total: bytes.len() as u32,
        digest: digest(),
        bytes: bytes[CHUNK_BYTES..].to_vec(),
    };
    for chunk in [&first, &second] {
        let frame = encode_chunk(chunk).unwrap();
        assert!(frame.len() <= FRAME_BYTES);
        assert_eq!(decode_chunk(&frame).unwrap(), *chunk);
        transfer.push(chunk).unwrap();
    }
    assert_eq!(transfer.finish().unwrap(), (bytes, digest()));

    let mut replay = BlobTransfer::new(2, digest(), 2).unwrap();
    let chunk = BlobChunk {
        offset: 1,
        total: 2,
        digest: digest(),
        bytes: b"b".to_vec(),
    };
    assert!(replay.push(&chunk).is_err());
    let partial = BlobTransfer::new(2, digest(), 2).unwrap();
    assert!(partial.finish().is_err());
}

#[test]
fn terminal_projection_requires_drain_and_only_builds_typed_task_finished() {
    let task = task_id();
    let initial = initial(&task, "completed");
    let facts = TerminalProjectionFacts {
        initial,
        exit_code: 0,
        finished_at: "2026-01-10T10:05:00+00:00",
        output_drained: true,
    };
    let (header, status) = terminal_projection(&facts).unwrap();
    assert_eq!(header, status);
    let event = task_finished(&facts, 1).unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&event).unwrap();
    assert_eq!(parsed["kind"], "task_finished");
    assert_eq!(parsed["status"], "completed");
    assert!(
        terminal_projection(&TerminalProjectionFacts {
            output_drained: false,
            ..facts
        })
        .is_err()
    );
}
