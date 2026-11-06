#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_application::owner_runtime::DigestPort;
use hf_backends::{
    claude_measurement::ClaudeFixtureRequest,
    claude_offline::{plan, reduce},
};
use hf_domain::{
    backend::{BackendBinding, BackendError, BackendRecord, BackendTrace, RecordKind},
    capture::FileIdentity,
    owner_policy::{
        AuthorityContext, AuthorityRole, PermissionMode, Requirement, ResultState, RuntimePolicy,
    },
    ownership::{Digest, Id},
    process::ProcessIdentity,
};

struct TestDigest;
impl DigestPort for TestDigest {
    fn digest_parts(&self, parts: &[&[u8]]) -> Digest {
        let mut state = 0xcbf29ce484222325u64;
        for part in parts {
            for byte in *part {
                state = (state ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
            }
        }
        Digest::new(format!("{state:016x}{}", "0".repeat(48))).unwrap()
    }
}

fn id(value: &str) -> Id {
    Id::new(format!("{value:0<32}")).unwrap()
}
fn record(kind: RecordKind, session: &str, parent: Option<&str>) -> BackendRecord {
    BackendRecord {
        cli_session: session.into(),
        attempt: id("a1b2c3d4e5f60718a1b2c3d4e5f60718"),
        parent: parent.map(str::to_owned),
        kind,
    }
}
fn trace(kinds: Vec<BackendRecord>) -> BackendTrace {
    BackendTrace {
        records: kinds,
        fault: None,
        ended: true,
    }
}
fn root_prefix() -> Vec<BackendRecord> {
    vec![record(RecordKind::Init, "cli-root", None)]
}
fn binding() -> BackendBinding {
    let attempt = id("a1b2c3d4e5f60718a1b2c3d4e5f60718");
    let payload = b"offline fixture payload".to_vec();
    BackendBinding {
        request: hf_domain::ownership::PreparedRequest {
            operation: id("11111111111111111111111111111111"),
            session: id("22222222222222222222222222222222"),
            reservation: id("33333333333333333333333333333333"),
            attempt: attempt.clone(),
            generation: id("44444444444444444444444444444444"),
            digest: TestDigest.digest_parts(&[&payload]),
            payload,
        },
        authority: AuthorityContext {
            caller: ProcessIdentity::new(42, 1, 0, "abcdef01-2345-6789-abcd-ef0123456789".into())
                .unwrap(),
            role: AuthorityRole::Direct,
            checkpoint: None,
            revision: None,
            issued_attempt: attempt,
            grant_digest: TestDigest.digest_parts(&[b"grant"]),
        },
        root: FileIdentity {
            device: 0,
            inode: 2,
        },
        profile: TestDigest.digest_parts(&[b"profile"]),
        policy: RuntimePolicy {
            background_requirement: Requirement::Required,
            permission_mode: PermissionMode::ToolLevel,
            background_idle_seconds: 1.0,
            tool_level_opt_in: true,
        },
        runtime_version: "2.1.318".into(),
        logical_executable: "/opt/claude".into(),
        physical_executable: "/opt/claude".into(),
        binary_digest: TestDigest.digest_parts(&[b"binary"]),
        configuration_digest: TestDigest.digest_parts(&[b"config"]),
    }
}

#[test]
fn root_terminal_is_correlated_and_child_terminal_cannot_complete_attempt() {
    let mut events = root_prefix();
    events.push(record(RecordKind::Text("root".into()), "cli-root", None));
    events.push(record(
        RecordKind::Terminal { failed: false },
        "cli-root",
        Some("child"),
    ));
    let child_only = reduce(&trace(events), &binding());
    assert!(!child_only.terminal);
    assert_eq!(child_only.result, ResultState::Unknown);

    let mut complete = root_prefix();
    complete.push(record(
        RecordKind::Terminal { failed: false },
        "cli-root",
        None,
    ));
    let summary = reduce(&trace(complete), &binding());
    assert_eq!(summary.result, ResultState::Succeeded);
}

#[test]
fn duplicate_init_foreign_session_and_attempt_are_unusable() {
    let expected = binding();
    let mut duplicate = root_prefix();
    duplicate.push(record(RecordKind::Init, "cli-root", None));
    duplicate.push(record(
        RecordKind::Terminal { failed: false },
        "cli-root",
        None,
    ));
    assert!(!reduce(&trace(duplicate), &expected).usable);

    let mut foreign = root_prefix();
    foreign.push(record(
        RecordKind::Terminal { failed: false },
        "cli-foreign",
        None,
    ));
    assert!(!reduce(&trace(foreign), &expected).usable);

    let mut wrong_attempt = root_prefix();
    let mut terminal = record(RecordKind::Terminal { failed: false }, "cli-root", None);
    terminal.attempt = id("ffffffffffffffffffffffffffffffff");
    wrong_attempt.push(terminal);
    assert!(!reduce(&trace(wrong_attempt), &expected).usable);
}

#[test]
fn duplicate_terminal_and_postterminal_activity_make_trace_unusable() {
    let expected = binding();
    let mut duplicate = root_prefix();
    duplicate.push(record(
        RecordKind::Terminal { failed: false },
        "cli-root",
        None,
    ));
    duplicate.push(record(
        RecordKind::Terminal { failed: false },
        "cli-root",
        None,
    ));
    assert!(!reduce(&trace(duplicate), &expected).usable);

    let mut postterminal = root_prefix();
    postterminal.push(record(
        RecordKind::Terminal { failed: false },
        "cli-root",
        None,
    ));
    postterminal.push(record(RecordKind::Text("late".into()), "cli-root", None));
    assert!(!reduce(&trace(postterminal), &expected).usable);
}

#[test]
fn tool_results_cannot_cross_parent_scope_and_duplicate_ids_refuse() {
    let expected = binding();
    let mut crossed = root_prefix();
    crossed.push(record(RecordKind::ToolUse("t1".into()), "cli-root", None));
    crossed.push(record(
        RecordKind::ToolResult {
            id: "t1".into(),
            is_error: false,
        },
        "cli-root",
        Some("child"),
    ));
    crossed.push(record(
        RecordKind::Terminal { failed: false },
        "cli-root",
        None,
    ));
    let summary = reduce(&trace(crossed), &expected);
    assert!(!summary.usable);
    assert_eq!(summary.pending_tools, 1);

    let mut duplicate = root_prefix();
    duplicate.push(record(RecordKind::ToolUse("t1".into()), "cli-root", None));
    duplicate.push(record(RecordKind::ToolUse("t1".into()), "cli-root", None));
    assert!(!reduce(&trace(duplicate), &expected).usable);

    let mut passive = root_prefix();
    passive.push(record(RecordKind::ToolUse("once".into()), "cli-root", None));
    passive.push(record(
        RecordKind::ToolResult {
            id: "once".into(),
            is_error: true,
        },
        "cli-root",
        None,
    ));
    passive.push(record(
        RecordKind::Terminal { failed: false },
        "cli-root",
        None,
    ));
    let summary = reduce(&trace(passive), &expected);
    assert!(summary.usable);
    assert_eq!(summary.pending_tools, 0);
    assert_eq!(summary.result, ResultState::Succeeded);

    let mut reuse = root_prefix();
    reuse.push(record(RecordKind::ToolUse("once".into()), "cli-root", None));
    reuse.push(record(
        RecordKind::ToolResult {
            id: "once".into(),
            is_error: false,
        },
        "cli-root",
        None,
    ));
    reuse.push(record(RecordKind::ToolUse("once".into()), "cli-root", None));
    assert!(!reduce(&trace(reuse), &expected).usable);
}

#[test]
fn child_text_is_observation_only_and_sticky_failure_survives_fault() {
    let expected = binding();
    let mut events = root_prefix();
    events.push(record(
        RecordKind::Text("child-secret".into()),
        "cli-root",
        Some("child"),
    ));
    events.push(record(
        RecordKind::Terminal { failed: true },
        "cli-root",
        None,
    ));
    let mut failed = trace(events);
    failed.fault = Some(BackendError::Shape);
    let summary = reduce(&failed, &expected);
    assert!(summary.sticky_failure);
    assert_eq!(summary.result, ResultState::Failed);
    assert!(summary.text.is_empty());
    assert!(!summary.usable);
}

fn request() -> ClaudeFixtureRequest {
    ClaudeFixtureRequest {
        logical_executable: "/opt/claude".into(),
        home: "/tmp/h".into(),
        config: "/tmp/c".into(),
        xdg_config: "/tmp/xc".into(),
        xdg_cache: "/tmp/xk".into(),
        temporary: "/tmp/t".into(),
        claude_temporary: "/tmp/ct".into(),
        port: 42123,
        nonce: "a1b2c3d4e5f60718".into(),
    }
}

#[test]
fn invocation_is_one_shot_prompt_is_option_delimited_and_digest_bound() {
    let first = plan(&request(), "-dangerous prompt", "/tmp/work", &TestDigest).unwrap();
    let changed = plan(&request(), "different prompt", "/tmp/work", &TestDigest).unwrap();
    let delimiter = first.args.iter().position(|arg| arg == "--").unwrap();
    assert_eq!(first.args.last().unwrap(), "-dangerous prompt");
    assert_eq!(delimiter + 1, first.args.len() - 1);
    assert_ne!(first.configuration_digest, changed.configuration_digest);
    assert_eq!(first.cwd, "/tmp/work");
    assert!(plan(&request(), "p", "/tmp/../outside", &TestDigest).is_err());
}
