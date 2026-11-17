#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Production launch metadata, stdin, chunk and projection contracts.
use hf_application::{
    task_lifecycle::InitialProjectionFacts,
    task_runtime::{
        InitialIntentValidator, InvalidIntent, SpawnProjectionFacts, SpawnProjectionValidator,
    },
};
use hf_domain::{
    capture::FileIdentity,
    ownership::{Digest, Id, PreparedRequest},
    prepared_launch::{BinaryMetadata, BinaryPin},
    process::ProcessIdentity,
    recovery::SourceStamp,
    task_launch::CliSessionId,
    task_lifecycle::LifecycleBinding,
    task_read::TaskId,
    task_runtime::{LaunchIntent, LaunchIntentCreate, ProductionInvitation},
};
use hf_interfaces::task_runtime::{
    CanonicalLaunchValidator, InitialLaunchMetadata, decode_invitation, decode_metadata,
    encode_invitation, encode_metadata, intent_payload, spawn_projection,
};

fn task_id() -> TaskId {
    TaskId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into()).unwrap()
}

fn cli_session() -> CliSessionId {
    CliSessionId::new("123e4567-e89b-12d3-a456-426614174000".into()).unwrap()
}

fn digest() -> Digest {
    Digest::new("a".repeat(64)).unwrap()
}

fn id(byte: u8) -> Id {
    Id::new(format!("{byte:032x}")).unwrap()
}

fn metadata(invitation: &ProductionInvitation) -> InitialLaunchMetadata {
    InitialLaunchMetadata {
        task: task_id(),
        session: invitation.session.clone(),
        repo: "/workspace/repo".into(),
        title: Some("Initial task".into()),
        model: None,
        created_at: "2026-01-10T10:00:00+00:00".into(),
        caller_reference: format!(
            "{}:{}:{}:{}",
            invitation.caller.pid(),
            invitation.caller.start_seconds(),
            invitation.caller.start_microseconds(),
            invitation.caller.boot_uuid()
        ),
        configuration_reference: invitation.configuration_digest.as_str().into(),
    }
}

fn initial<'a>(task: &'a TaskId, status: &'a str) -> InitialProjectionFacts<'a> {
    InitialProjectionFacts {
        task,
        backend: "claude",
        repo_path: "/workspace/repo",
        session_id: "123e4567-e89b-12d3-a456-426614174000",
        status,
        started_at: "2026-01-10T10:00:00+00:00",
        title: Some("Initial task"),
        model: None,
        continuation_of: None,
        chain_root: None,
    }
}

fn invitation() -> ProductionInvitation {
    let binding = LifecycleBinding {
        task: task_id(),
        operation: id(1),
        attempt: id(2),
        generation: id(3),
    };
    let source = SourceStamp {
        generation: binding.generation.clone(),
        root: FileIdentity {
            device: 1,
            inode: 2,
        },
        database: FileIdentity {
            device: 1,
            inode: 3,
        },
        marker: FileIdentity {
            device: 1,
            inode: 4,
        },
        store_lock: FileIdentity {
            device: 1,
            inode: 5,
        },
        io_lock: FileIdentity {
            device: 1,
            inode: 6,
        },
    };
    let binary = |name: &str, inode| BinaryPin {
        logical_path: format!("/usr/local/bin/{name}"),
        physical_path: format!("/opt/harflow/{name}"),
        metadata: BinaryMetadata {
            identity: FileIdentity { device: 1, inode },
            length: 4096,
            modified_seconds: 1_700_000_000,
            modified_nanoseconds: 0,
            changed_seconds: 1_700_000_000,
            changed_nanoseconds: 0,
        },
        digest: digest(),
    };
    ProductionInvitation {
        binding,
        reservation: id(4),
        ownership_session: id(5),
        session: cli_session(),
        source,
        caller: ProcessIdentity::new(
            4500,
            1_768_000_000,
            1,
            "01234567-89ab-cdef-0123-456789abcdef".into(),
        )
        .unwrap(),
        dispatcher: ProcessIdentity::new(
            4501,
            1_768_000_001,
            2,
            "01234567-89ab-cdef-0123-456789abcdef".into(),
        )
        .unwrap(),
        ctl: binary("harflow-ctl", 7),
        backend: binary("claude", 8),
        cwd: "/workspace/repo".into(),
        environment_digest: digest(),
        prompt_digest: digest(),
        metadata_digest: digest(),
        configuration_digest: digest(),
        invocation_digest: digest(),
        expected_version: "2.1.322".into(),
    }
}

fn request(invitation: &ProductionInvitation) -> PreparedRequest {
    let payload = encode_invitation(invitation).unwrap();
    PreparedRequest {
        operation: invitation.binding.operation.clone(),
        session: invitation.ownership_session.clone(),
        reservation: invitation.reservation.clone(),
        attempt: invitation.binding.attempt.clone(),
        generation: invitation.binding.generation.clone(),
        digest: digest(),
        payload,
    }
}

#[test]
fn metadata_roundtrips_canonically_and_rejects_duplicate_or_unknown_fields() {
    let invitation = invitation();
    let value = metadata(&invitation);
    let encoded = encode_metadata(&value).unwrap();
    assert!(encoded.len() <= 16_384);
    assert_eq!(decode_metadata(&encoded).unwrap(), value);

    let text = String::from_utf8(encoded.clone()).unwrap();
    let duplicate = text.strip_suffix('}').unwrap().to_owned()
        + ",\"task\":\"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb\"}";
    assert_eq!(decode_metadata(duplicate.as_bytes()), Err(InvalidIntent));
    let unknown = text.strip_suffix('}').unwrap().to_owned() + ",\"extra\":true}";
    assert_eq!(decode_metadata(unknown.as_bytes()), Err(InvalidIntent));
    assert_eq!(
        encode_metadata(&InitialLaunchMetadata {
            created_at: "2026-01-10T10:00:00".into(),
            ..value
        }),
        Err(InvalidIntent)
    );
}

#[test]
fn canonical_t0_validator_binds_metadata_task_and_entire_preparation_identity() {
    let invitation = invitation();
    let metadata = metadata(&invitation);
    let metadata_bytes = encode_metadata(&metadata).unwrap();
    let create = LaunchIntentCreate {
        task: metadata.task.clone(),
        metadata_digest: invitation.metadata_digest.clone(),
        metadata: metadata_bytes,
        prompt: "Summarize the repo".into(),
        prompt_digest: invitation.prompt_digest.clone(),
    };
    let request = request(&invitation);
    let validator = CanonicalLaunchValidator;
    assert!(InitialIntentValidator::validate(&validator, &request, &create).is_ok());
    assert_eq!(decode_invitation(&request.payload).unwrap(), invitation);
    let mut trailing = request.payload.clone();
    trailing.push(0);
    assert_eq!(decode_invitation(&trailing), Err(InvalidIntent));
    let mut wrong_source = invitation.clone();
    wrong_source.source.generation = id(9);
    assert_eq!(encode_invitation(&wrong_source), Err(InvalidIntent));

    let mut changed_attempt = request.clone();
    changed_attempt.attempt = id(6);
    assert_eq!(
        InitialIntentValidator::validate(&validator, &changed_attempt, &create),
        Err(InvalidIntent)
    );
    let mut wrong_task = create.clone();
    wrong_task.task = TaskId::new("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into()).unwrap();
    assert_eq!(
        InitialIntentValidator::validate(&validator, &request, &wrong_task),
        Err(InvalidIntent)
    );
    let mut legacy_request = request.clone();
    legacy_request.payload = intent_payload(&request, &create);
    assert_eq!(
        InitialIntentValidator::validate(&validator, &legacy_request, &create),
        Err(InvalidIntent)
    );

    let mut changed_metadata = metadata.clone();
    changed_metadata.configuration_reference = "different-config".into();
    let wrong_intent = LaunchIntentCreate {
        metadata: encode_metadata(&changed_metadata).unwrap(),
        ..create
    };
    assert_eq!(
        InitialIntentValidator::validate(&validator, &request, &wrong_intent),
        Err(InvalidIntent)
    );
}

#[test]
fn spawn_projection_carries_canonical_unavailable_summaries() {
    let task = task_id();
    let facts = initial(&task, "running");
    let (header, status) = spawn_projection(&facts).unwrap();
    assert_eq!(header, status);
    let header_json: serde_json::Value = serde_json::from_slice(&header).unwrap();
    assert_eq!(header_json["status"], "running");
    assert_eq!(header_json["freedom"], "unrestricted");
    assert_eq!(header_json["enforcement"]["os_enforced"], false);
    assert_eq!(
        header_json["queued_followups"],
        serde_json::json!({"state":"unavailable","items":[],"omitted_final":0})
    );
    assert_eq!(
        header_json["artifact_summary"],
        serde_json::json!({"count":0,"state":"unavailable"})
    );
    assert_eq!(
        header_json["approval_summary"],
        serde_json::json!({
            "pending_count":0,"awaiting_decision_count":0,"delivery_uncertain_count":0,
            "interaction_state":"approval_state_uncertain","requests":[],"uncertainty_requests":[],
            "error":"Approval ledger unavailable; execution must be reconciled"
        })
    );
    assert_eq!(
        header_json["recovery_activity"],
        serde_json::json!({
            "source":"backend_transcript","display_only":true,"events":[],"next_cursor":null,
            "has_more":false,"reset":false,"notices":["Transcript recovery requires confirmed capture loss"]
        })
    );
    assert!(spawn_projection(&initial(&task, "completed")).is_err());
}

#[test]
fn spawn_validator_binds_intent_and_requires_exact_task_facts() {
    let task = task_id();
    let facts = initial(&task, "running");
    let (header, status) = spawn_projection(&facts).unwrap();
    let invitation = invitation();
    let metadata = metadata(&invitation);
    let create = LaunchIntentCreate {
        task: metadata.task.clone(),
        metadata: encode_metadata(&metadata).unwrap(),
        metadata_digest: invitation.metadata_digest.clone(),
        prompt: "Summarize the repo".into(),
        prompt_digest: invitation.prompt_digest.clone(),
    };
    let intent = LaunchIntent {
        binding: invitation.binding.clone(),
        preparation: request(&invitation),
        preparation_digest: digest(),
        create,
    };
    let child = ProcessIdentity::new(
        5100,
        1_768_000_000,
        123_456,
        "01234567-89ab-cdef-0123-456789abcdef".into(),
    )
    .unwrap();
    let validator = CanonicalLaunchValidator;
    let spawn_facts = SpawnProjectionFacts {
        initial: facts,
        child: &child,
    };
    assert!(
        SpawnProjectionValidator::validate(
            &validator,
            &intent,
            &spawn_facts,
            &header,
            &status,
            &intent.create.prompt
        )
        .is_ok()
    );
    let mut forged = initial(&task, "running");
    forged.session_id = "foreign-session";
    let forged = SpawnProjectionFacts {
        initial: forged,
        child: &child,
    };
    assert_eq!(
        SpawnProjectionValidator::validate(
            &validator,
            &intent,
            &forged,
            &header,
            &status,
            &intent.create.prompt
        ),
        Err(InvalidIntent)
    );
    let extra = String::from_utf8(header)
        .unwrap()
        .replace("{", "{\"extra\":true,");
    assert_eq!(
        SpawnProjectionValidator::validate(
            &validator,
            &intent,
            &spawn_facts,
            extra.as_bytes(),
            &status,
            &intent.create.prompt
        ),
        Err(InvalidIntent)
    );
}

#[path = "task_runtime/contracts.rs"]
mod contracts;
#[path = "task_runtime/writer_codecs.rs"]
mod writer;
