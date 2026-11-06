#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_application::{
    backend_runtime::{self, IndependentFacts, OwnerObservation, OwnerObservationPort},
    owner_runtime::DigestPort,
};
use hf_backends::{
    claude_measurement::ClaudeFixtureRequest,
    claude_offline::{RecordedAdapter, plan, reduce},
};
use hf_domain::{
    backend::*,
    capture::{CaptureBinding, CaptureProgress, CaptureSnapshot, FileIdentity},
    owner_policy::*,
    ownership::{Id, OwnerClaim, Phase, PreparedRequest, Snapshot},
    process::{ProcessExit, ProcessIdentity},
};
use hf_interfaces::claude_stream::ClaudeStreamDecoder;
use hf_storage::Sha256Digest;

fn id(value: &str) -> Id {
    Id::new(format!("{value:0<32}")).unwrap()
}
fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 1, 0, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}
fn binding(digest: &impl DigestPort) -> BackendBinding {
    let attempt = id("a1b2c3d4e5f60718a1b2c3d4e5f60718");
    let payload = b"synthetic prepared payload".to_vec();
    BackendBinding {
        request: PreparedRequest {
            operation: id("11111111111111111111111111111111"),
            session: id("22222222222222222222222222222222"),
            reservation: id("33333333333333333333333333333333"),
            attempt: attempt.clone(),
            generation: id("44444444444444444444444444444444"),
            digest: digest.digest_parts(&[&payload]),
            payload,
        },
        authority: AuthorityContext {
            caller: process(42),
            role: AuthorityRole::Direct,
            checkpoint: None,
            revision: None,
            issued_attempt: attempt,
            grant_digest: digest.digest_parts(&[b"synthetic grant"]),
        },
        root: FileIdentity {
            device: 0,
            inode: 9,
        },
        profile: digest.digest_parts(&[b"synthetic profile"]),
        policy: RuntimePolicy {
            background_requirement: Requirement::Required,
            permission_mode: PermissionMode::ToolLevel,
            background_idle_seconds: 10.0,
            tool_level_opt_in: true,
        },
        runtime_version: "2.1.318".into(),
        logical_executable: "/opt/claude".into(),
        physical_executable: "/opt/claude".into(),
        binary_digest: digest.digest_parts(&[b"synthetic binary"]),
        configuration_digest: digest.digest_parts(&[b"pending config"]),
    }
}
struct Owner(OwnerObservation);
impl OwnerObservationPort for Owner {
    fn owner_observation(&self) -> OwnerObservation {
        self.0.clone()
    }
}
fn owner(binding: &BackendBinding, stdout_len: u64, stderr_len: u64) -> Owner {
    let owner = OwnerClaim {
        owner: id("55555555555555555555555555555555"),
        nonce: id("66666666666666666666666666666666"),
        process: process(43),
    };
    let child = process(44);
    let snapshot = Snapshot {
        request: binding.request.clone(),
        phase: Phase::AcceptanceRecorded,
        revision: 3,
        cancelled: false,
        owner: Some(owner),
        child: Some(child.clone()),
    };
    let capture = CaptureSnapshot {
        binding: CaptureBinding {
            fence: snapshot.fence().unwrap(),
            child,
            stdout: FileIdentity {
                device: 1,
                inode: 1,
            },
            stderr: FileIdentity {
                device: 1,
                inode: 2,
            },
        },
        progress: CaptureProgress {
            revision: 2,
            stdout: stdout_len,
            stderr: stderr_len,
            stdout_eof: true,
            stderr_eof: true,
            loss: false,
        },
        exit: ProcessExit::Code(0),
    };
    Owner(OwnerObservation {
        snapshot: Some(snapshot),
        capture: Some(capture),
        poisoned: false,
        pending: false,
    })
}

#[test]
fn synthetic_host_composition_uses_sha_and_keeps_owner_evidence_independent() {
    let digest = Sha256Digest;
    let mut binding = binding(&digest);
    let request = ClaudeFixtureRequest {
        logical_executable: binding.logical_executable.clone(),
        home: "/tmp/home".into(),
        config: "/tmp/config".into(),
        xdg_config: "/tmp/xdg-config".into(),
        xdg_cache: "/tmp/xdg-cache".into(),
        temporary: "/tmp/tmp".into(),
        claude_temporary: "/tmp/claude-tmp".into(),
        port: 42123,
        nonce: "a1b2c3d4e5f60718".into(),
    };
    let invocation = plan(&request, "synthetic prompt", "/tmp/work", &digest).unwrap();
    binding.configuration_digest = invocation.configuration_digest.clone();
    let lines = concat!(
        "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"cli-synthetic\"}\n",
        "{\"type\":\"assistant\",\"session_id\":\"cli-synthetic\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"t1\"}]}}\n",
        "{\"type\":\"user\",\"session_id\":\"cli-synthetic\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"is_error\":true}]}}\n",
        "{\"type\":\"result\",\"session_id\":\"cli-synthetic\",\"subtype\":\"success\",\"is_error\":false}\n"
    );
    let mut decoder = ClaudeStreamDecoder::new(binding.request.attempt.clone());
    for chunk in lines.as_bytes().chunks(31) {
        decoder.push(chunk);
    }
    let parsed = decoder.finish();
    let mut adapter = RecordedAdapter {
        trace: Some(parsed),
    };
    let observed = backend_runtime::observe(
        &binding,
        &binding,
        &invocation,
        BackendMode::OfflineOneShot,
        &digest,
        &mut adapter,
    )
    .unwrap();
    let summary = reduce(&observed, &binding);
    assert!(summary.usable);
    assert_eq!(summary.pending_tools, 0);
    let facts = IndependentFacts {
        binding: binding.clone(),
        input: InputState::Reconciled,
        inventory: InventoryState::Complete,
        obligations: ObligationState::Satisfied,
        watermark: WatermarkState::Complete,
        containment: ContainmentState::Complete,
        writer: WriterState::Settled,
        claims: ClaimsState::Reconciled,
        execution: ExecutionState::Healthy,
    };
    let result = backend_runtime::project(
        &binding,
        &summary,
        &facts,
        &owner(&binding, lines.len() as u64, 0),
    );
    assert_eq!(result.workflow, WorkflowState::Succeeded);
    assert_eq!(result.release, ReleaseEvidence::EvidenceComplete);
}
