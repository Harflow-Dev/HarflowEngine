#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::{initial, task_id};
use hf_application::task_runtime::TerminalProjectionValidator;
use hf_application::task_runtime::{InvalidIntent, TerminalProjectionFacts};
use hf_domain::{
    ownership::{Digest, Id, Phase, PreparedRequest, Snapshot},
    task_lifecycle::{ExecutionObservation, LifecycleBinding, LifecycleEvidence, ResultWitness},
};
use hf_interfaces::task_runtime::{
    BlobChunk, BlobTransfer, CHUNK_BYTES, CanonicalTerminalSummaryProjection, FRAME_BYTES,
    decode_chunk, encode_chunk, encode_live_message, successful_result_summary, task_finished,
    task_finished_observed, terminal_projection, terminal_projection_with_summary,
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

#[test]
fn terminal_summary_reports_only_the_durable_normalized_omission_count() {
    let task = task_id();
    let facts = TerminalProjectionFacts {
        initial: initial(&task, "completed"),
        exit_code: 0,
        finished_at: "2026-01-10T10:05:00+00:00",
        output_drained: true,
    };
    let mut observation = ExecutionObservation::default();
    let no_omissions = task_finished_observed(&facts, 7, &observation).unwrap();
    let ordinary = task_finished(&facts, 7).unwrap();
    assert_eq!(no_omissions, ordinary);

    observation.omitted_events = 3;
    let disclosed = task_finished_observed(&facts, 7, &observation).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&disclosed).unwrap();
    assert_eq!(document["summary"], "3 normalized events omitted.");
    assert_eq!(document["seq"], 7);
    assert_eq!(document["task_id"], task.as_str());
}

#[test]
fn result_summary_accepts_only_bounded_successful_root_text() {
    let text = "Résumé ☕ — 日本語";
    let success = serde_json::json!({
        "type": "result",
        "subtype": "success",
        "is_error": false,
        "result": text
    });
    assert_eq!(
        successful_result_summary(&serde_json::to_vec(&success).unwrap()).unwrap(),
        Some(text.to_owned())
    );

    let error = serde_json::json!({
        "type": "result",
        "subtype": "error",
        "is_error": true,
        "result": "partial"
    });
    assert_eq!(
        successful_result_summary(&serde_json::to_vec(&error).unwrap()).unwrap(),
        None
    );
    let child = serde_json::json!({
        "type": "result",
        "subtype": "success",
        "is_error": false,
        "parent_tool_use_id": "toolu_child",
        "result": "nested"
    });
    assert_eq!(
        successful_result_summary(&serde_json::to_vec(&child).unwrap()).unwrap(),
        None
    );
    let oversized = serde_json::json!({
        "type": "result",
        "subtype": "success",
        "is_error": false,
        "result": "x".repeat(16_385)
    });
    assert_eq!(
        successful_result_summary(&serde_json::to_vec(&oversized).unwrap()).unwrap(),
        None
    );
}

#[test]
fn oversized_summary_falls_back_to_the_bounded_baseline_projection() {
    let task = task_id();
    let facts = TerminalProjectionFacts {
        initial: initial(&task, "completed"),
        exit_code: 0,
        finished_at: "2026-01-10T10:05:00+00:00",
        output_drained: true,
    };
    let baseline = terminal_projection(&facts).unwrap();
    let (header, status) =
        terminal_projection_with_summary(&facts, Some(&"x".repeat(14_000))).unwrap();
    assert_eq!(header, baseline.0);
    assert_eq!(status, baseline.1);
    assert!(status.len() <= 16_384);
    let document: serde_json::Value = serde_json::from_slice(&status).unwrap();
    assert_eq!(document["summary"], serde_json::Value::Null);
}

#[test]
fn canonical_summary_projection_requires_the_committed_success_witness() {
    let task = task_id();
    let operation = Id::new("1".repeat(32)).unwrap();
    let attempt = Id::new("2".repeat(32)).unwrap();
    let generation = Id::new("3".repeat(32)).unwrap();
    let request = PreparedRequest {
        operation: operation.clone(),
        session: Id::new("4".repeat(32)).unwrap(),
        reservation: Id::new("5".repeat(32)).unwrap(),
        attempt: attempt.clone(),
        generation: generation.clone(),
        digest: digest(),
        payload: Vec::new(),
    };
    let witness = ResultWitness {
        turn: attempt.clone(),
        digest: digest(),
        source_end: 123,
        success: true,
    };
    let observation = ExecutionObservation {
        result: Some(witness.clone()),
        ..ExecutionObservation::default()
    };
    let evidence = LifecycleEvidence {
        binding: LifecycleBinding {
            task: task.clone(),
            operation,
            attempt,
            generation,
        },
        revision: 2,
        ownership: Snapshot {
            request,
            phase: Phase::SpawnObserved,
            revision: 2,
            cancelled: false,
            owner: None,
            child: None,
        },
        cancel: None,
        observation,
    };
    let facts = TerminalProjectionFacts {
        initial: initial(&task, "completed"),
        exit_code: 0,
        finished_at: "2026-01-10T10:05:00+00:00",
        output_drained: true,
    };
    let summary = "Exact result text: café 日本語";
    let (header, status) = terminal_projection_with_summary(&facts, Some(summary)).unwrap();
    let event = task_finished_observed(&facts, 7, &evidence.observation).unwrap();
    let validator = CanonicalTerminalSummaryProjection {
        summary,
        result: &witness,
    };
    assert_eq!(
        validator.validate(&evidence, &facts, &header, &status, &event, 7),
        Ok(())
    );

    let mut changed_witness = witness;
    changed_witness.digest = Digest::new("b".repeat(64)).unwrap();
    let mismatched = CanonicalTerminalSummaryProjection {
        summary,
        result: &changed_witness,
    };
    assert!(
        mismatched
            .validate(&evidence, &facts, &header, &status, &event, 7)
            .is_err()
    );
}
