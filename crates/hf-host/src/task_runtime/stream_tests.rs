#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::stream::Stream;
use hf_application::{
    task_lifecycle::InitialProjectionFacts, task_runtime::TerminalProjectionFacts,
};
use hf_domain::{
    capture::FileIdentity,
    ownership::{Digest, Id},
    prepared_launch::{BinaryMetadata, BinaryPin},
    recovery::SourceStamp,
    task_launch::CliSessionId,
    task_lifecycle::ExecutionObservation,
    task_read::TaskId,
    task_runtime::ProductionInvitation,
};

const SESSION: &str = "123e4567-e89b-12d3-a456-426614174000";
const BOOT: &str = "01234567-89ab-cdef-0123-456789abcdef";

fn id(byte: u8) -> Id {
    Id::new(format!("{byte:032x}")).unwrap()
}

fn digest(byte: char) -> Digest {
    Digest::new(byte.to_string().repeat(64)).unwrap()
}

fn invitation() -> ProductionInvitation {
    let binding = hf_domain::task_lifecycle::LifecycleBinding {
        task: TaskId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into()).unwrap(),
        operation: id(1),
        attempt: id(2),
        generation: id(3),
    };
    let file = |inode| FileIdentity { device: 1, inode };
    let binary = |name: &str, inode| BinaryPin {
        logical_path: format!("/usr/local/bin/{name}"),
        physical_path: format!("/opt/harflow/{name}"),
        metadata: BinaryMetadata {
            identity: file(inode),
            length: 4096,
            modified_seconds: 1_700_000_000,
            modified_nanoseconds: 0,
            changed_seconds: 1_700_000_000,
            changed_nanoseconds: 0,
        },
        digest: digest('a'),
    };
    ProductionInvitation {
        binding: binding.clone(),
        reservation: id(4),
        ownership_session: id(5),
        session: CliSessionId::new(SESSION.into()).unwrap(),
        source: SourceStamp {
            generation: binding.generation,
            root: file(2),
            database: file(3),
            marker: file(4),
            store_lock: file(5),
            io_lock: file(6),
        },
        caller: process(4500),
        dispatcher: process(4501),
        ctl: binary("harflow-ctl", 7),
        backend: binary("claude", 8),
        cwd: "/workspace/repo".into(),
        environment_digest: digest('b'),
        prompt_digest: digest('c'),
        metadata_digest: digest('d'),
        configuration_digest: digest('e'),
        invocation_digest: digest('f'),
        expected_version: "2.1.322".into(),
    }
}

fn process(pid: u32) -> hf_domain::process::ProcessIdentity {
    hf_domain::process::ProcessIdentity::new(pid, 1_768_000_000, u64::from(pid), BOOT.into())
        .unwrap()
}

fn line(value: String) -> Vec<u8> {
    let mut bytes = value.into_bytes();
    bytes.push(b'\n');
    bytes
}

fn init(stream: &mut Stream, observation: &mut ExecutionObservation) {
    let bytes = line(format!(
        r#"{{"type":"system","subtype":"init","session_id":"{SESSION}","tools":[{{"name":"Read"}}]}}"#
    ));
    stream.consume(&bytes, observation).unwrap();
}

#[test]
fn oversized_projection_keeps_bounded_identity_and_stream_continues() {
    let mut stream = Stream::new(&invitation()).unwrap();
    let mut observation = ExecutionObservation::default();
    init(&mut stream, &mut observation);
    let oversized = line(format!(
        r#"{{"type":"assistant","session_id":"{SESSION}","timestamp":"{}","message":{{"id":"m1","content":[{{"type":"text","text":"kept"}}]}}}}"#,
        "t".repeat(20_000)
    ));
    let projected = stream.consume(&oversized, &mut observation).unwrap();
    assert_eq!(projected.len(), 1);
    assert!(projected[0].body.len() <= 16_384);
    let stub = std::str::from_utf8(&projected[0].body).unwrap();
    assert!(stub.contains("\"oversized\":true"));
    assert!(stub.contains("\"truncated\":true"));
    assert!(stub.contains("\"kind\":\"assistant_text\""));
    assert!(stub.contains("\"format\":\"harflow.event\""));
    assert!(stub.contains(&format!("\"seq\":{}", projected[0].seq)));
    assert!(observation.event_projection_truncated);

    let small = line(format!(
        r#"{{"type":"assistant","session_id":"{SESSION}","message":{{"id":"m2","content":[{{"type":"text","text":"still flowing"}}]}}}}"#
    ));
    let after = stream.consume(&small, &mut observation).unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].seq, projected[0].seq + 1);
}

#[test]
fn one_hundred_projected_events_are_bounded_with_contiguous_sequences_and_t5_reserve() {
    let mut stream = Stream::new(&invitation()).unwrap();
    let mut observation = ExecutionObservation::default();
    init(&mut stream, &mut observation);
    let mut batch = Vec::new();
    for index in 0..100 {
        batch.extend(line(format!(
            r#"{{"type":"assistant","session_id":"{SESSION}","message":{{"id":"m{index}","content":[{{"type":"text","text":"short"}}]}}}}"#
        )));
    }
    let events = stream.consume(&batch, &mut observation).unwrap();
    assert_eq!(events.len(), 64);
    assert_eq!(events.first().unwrap().seq, 2);
    assert_eq!(events.last().unwrap().seq, 65);
    assert!(events.windows(2).all(|pair| pair[1].seq == pair[0].seq + 1));
    assert_eq!(stream.next_seq(), 66);
    assert_eq!(observation.omitted_events, 36);
    assert!(observation.observation_gap && observation.event_projection_truncated);
    let total_bytes: u64 = events.iter().map(|event| event.raw_len + 1).sum();
    assert!(total_bytes <= 64 * 1024 * 1024 - 131_072);
}

#[test]
fn projection_cap_preserves_reserved_bytes_for_t5_terminal_publication() {
    let mut stream = Stream::new(&invitation()).unwrap();
    let mut observation = ExecutionObservation::default();
    init(&mut stream, &mut observation);
    observation.normalized_event_bytes = 64 * 1024 * 1024 - 131_072 - 100;
    let short = line(format!(
        r#"{{"type":"assistant","session_id":"{SESSION}","message":{{"content":[{{"type":"text","text":"short"}}]}}}}"#
    ));
    let events = stream.consume(&short, &mut observation).unwrap();
    assert!(events.is_empty());
    assert_eq!(observation.omitted_events, 1);
    assert!(observation.observation_gap && observation.event_projection_truncated);
    assert!(observation.normalized_event_bytes < 64 * 1024 * 1024 - 65_536);
}

#[test]
fn recoverable_tool_error_is_kept_in_event_but_successful_root_result_succeeds() {
    let mut stream = Stream::new(&invitation()).unwrap();
    let mut observation = ExecutionObservation::default();
    init(&mut stream, &mut observation);
    let call = line(format!(
        r#"{{"type":"assistant","session_id":"{SESSION}","message":{{"content":[{{"type":"tool_use","id":"call-1","name":"Read","input":{{"path":"/missing"}}}}]}}}}"#
    ));
    assert_eq!(stream.consume(&call, &mut observation).unwrap().len(), 1);
    let tool_error = line(format!(
        r#"{{"type":"user","session_id":"{SESSION}","message":{{"content":[{{"type":"tool_result","tool_use_id":"call-1","is_error":true,"content":"not found"}}]}}}}"#
    ));
    let event = stream.consume(&tool_error, &mut observation).unwrap();
    assert_eq!(event.len(), 1);
    let parsed = std::str::from_utf8(&event[0].body).unwrap();
    assert!(parsed.contains("\"kind\":\"tool_result\""));
    assert!(parsed.contains("\"ok\":false"));
    let result = line(format!(
        r#"{{"type":"result","session_id":"{SESSION}","subtype":"success","is_error":false}}"#
    ));
    stream.consume(&result, &mut observation).unwrap();
    assert!(observation.result.as_ref().unwrap().success);
    assert!(stream.close_input());
}

#[test]
fn successful_root_result_preserves_exact_unicode_summary() {
    let mut stream = Stream::new(&invitation()).unwrap();
    let mut observation = ExecutionObservation::default();
    init(&mut stream, &mut observation);
    let summary = "Résumé: café ☕ — 日本語";
    let result = line(format!(
        r#"{{"type":"result","session_id":"{SESSION}","subtype":"success","is_error":false,"result":"{summary}"}}"#
    ));
    stream.consume(&result, &mut observation).unwrap();

    let (text, witness) = stream.summary().unwrap();
    assert_eq!(text, summary);
    assert!(witness.success);
    assert_eq!(observation.result.as_ref(), Some(witness));

    let task = invitation().binding.task;
    let facts = TerminalProjectionFacts {
        initial: InitialProjectionFacts {
            task: &task,
            backend: "claude",
            repo_path: "/workspace/repo",
            session_id: SESSION,
            status: "completed",
            started_at: "2026-01-10T10:00:00+00:00",
            title: None,
            model: None,
            continuation_of: None,
            chain_root: None,
        },
        exit_code: 0,
        finished_at: "2026-01-10T10:05:00+00:00",
        output_drained: true,
    };
    let (_, status) =
        hf_interfaces::task_runtime::terminal_projection_with_summary(&facts, Some(text)).unwrap();
    let expected = format!(r#""summary":"{summary}""#);
    assert!(
        status
            .windows(expected.len())
            .any(|window| window == expected.as_bytes())
    );
    assert!(status.len() <= 16_384);
}

#[test]
fn failed_root_result_never_produces_a_success_summary() {
    let mut stream = Stream::new(&invitation()).unwrap();
    let mut observation = ExecutionObservation::default();
    init(&mut stream, &mut observation);
    let result = line(format!(
        r#"{{"type":"result","session_id":"{SESSION}","subtype":"error_during_execution","is_error":true,"result":"partial output"}}"#
    ));
    stream.consume(&result, &mut observation).unwrap();

    assert!(stream.summary().is_none());
    assert!(!observation.result.as_ref().unwrap().success);
}
