#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_application::{
    backend_runtime::{
        self, BackendObservationPort, IndependentFacts, OwnerObservation, OwnerObservationPort,
    },
    owner_runtime::DigestPort,
};
use hf_domain::{
    backend::*,
    capture::{CaptureBinding, CaptureProgress, CaptureSnapshot, FileIdentity},
    owner_policy::*,
    ownership::{Digest, Id, OwnerClaim, Phase, PreparedRequest, Snapshot},
    process::{ProcessExit, ProcessIdentity},
};

struct DigestFixture;
impl DigestPort for DigestFixture {
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
fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 1, 0, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}
fn binding() -> BackendBinding {
    let attempt = id("a1b2c3d4e5f60718a1b2c3d4e5f60718");
    let payload = b"payload".to_vec();
    BackendBinding {
        request: PreparedRequest {
            operation: id("11111111111111111111111111111111"),
            session: id("22222222222222222222222222222222"),
            reservation: id("33333333333333333333333333333333"),
            attempt: attempt.clone(),
            generation: id("44444444444444444444444444444444"),
            digest: DigestFixture.digest_parts(&[&payload]),
            payload,
        },
        authority: AuthorityContext {
            caller: process(42),
            role: AuthorityRole::Direct,
            checkpoint: None,
            revision: None,
            issued_attempt: attempt,
            grant_digest: DigestFixture.digest_parts(&[b"grant"]),
        },
        root: FileIdentity {
            device: 0,
            inode: 9,
        },
        profile: DigestFixture.digest_parts(&[b"profile"]),
        policy: RuntimePolicy {
            background_requirement: Requirement::Required,
            permission_mode: PermissionMode::ToolLevel,
            background_idle_seconds: 10.0,
            tool_level_opt_in: true,
        },
        runtime_version: "2.1.318".into(),
        logical_executable: "/opt/claude".into(),
        physical_executable: "/opt/claude".into(),
        binary_digest: DigestFixture.digest_parts(&[b"binary"]),
        configuration_digest: DigestFixture.digest_parts(&[b"configuration"]),
    }
}
fn plan(binding: &BackendBinding) -> InvocationPlan {
    let mut plan = InvocationPlan {
        executable: binding.logical_executable.clone(),
        args: vec!["--".into(), "prompt".into()],
        env: vec![],
        cwd: "/tmp/work".into(),
        source_profile: "synthetic-offline-profile".into(),
        configuration_digest: DigestFixture.digest_parts(&[b"unused"]),
    };
    plan.configuration_digest = backend_runtime::configuration_digest(&plan, &DigestFixture);
    plan
}
struct Port {
    calls: usize,
}
impl BackendObservationPort for Port {
    fn observe(&mut self) -> Result<BackendTrace, BackendError> {
        self.calls += 1;
        Ok(BackendTrace {
            records: vec![],
            fault: None,
            ended: true,
        })
    }
}

#[test]
fn mismatched_context_and_every_production_mode_refuse_before_port_effects() {
    let mut expected = binding();
    let plan = plan(&expected);
    expected.configuration_digest = plan.configuration_digest.clone();
    let mut port = Port { calls: 0 };
    assert!(
        backend_runtime::observe(
            &expected,
            &expected,
            &plan,
            BackendMode::OfflineOneShot,
            &DigestFixture,
            &mut port
        )
        .is_ok()
    );
    assert_eq!(port.calls, 1);
    port.calls = 0;
    assert_production_modes_refuse(&expected, &plan, &mut port);
    for observed in mismatched_bindings(&expected) {
        assert_context_refused(&expected, &observed, &plan, &mut port);
    }
    assert_invalid_inputs_refuse(&expected, &plan, &mut port);
    assert_eq!(port.calls, 0);
}

fn assert_production_modes_refuse(
    expected: &BackendBinding,
    plan: &InvocationPlan,
    port: &mut Port,
) {
    for mode in [
        BackendMode::Live,
        BackendMode::Resume,
        BackendMode::Production,
    ] {
        assert_eq!(
            backend_runtime::observe(expected, expected, plan, mode, &DigestFixture, port),
            Err(BackendError::Unsupported)
        );
    }
}

fn mismatched_bindings(expected: &BackendBinding) -> Vec<BackendBinding> {
    let mut cases = Vec::new();
    let mut caller = expected.clone();
    caller.authority.caller = process(43);
    cases.push(caller);
    let mut grant = expected.clone();
    grant.authority.grant_digest = DigestFixture.digest_parts(&[b"other grant"]);
    cases.push(grant);
    let mut binary = expected.clone();
    binary.binary_digest = DigestFixture.digest_parts(&[b"other binary"]);
    cases.push(binary);
    let mut config = expected.clone();
    config.configuration_digest = DigestFixture.digest_parts(&[b"other configuration"]);
    cases.push(config);
    let mut request = expected.clone();
    request.request.generation = id("77777777777777777777777777777777");
    cases.push(request);
    let mut payload = expected.clone();
    payload.request.payload[0] ^= 1;
    cases.push(payload);
    let mut root = expected.clone();
    root.root.inode += 1;
    cases.push(root);
    let mut profile = expected.clone();
    profile.profile = DigestFixture.digest_parts(&[b"other profile"]);
    cases.push(profile);
    let mut runtime = expected.clone();
    runtime.runtime_version = "2.1.317".into();
    cases.push(runtime);
    let mut policy = expected.clone();
    policy.policy.background_idle_seconds += 1.0;
    cases.push(policy);
    cases
}

fn assert_context_refused(
    expected: &BackendBinding,
    observed: &BackendBinding,
    plan: &InvocationPlan,
    port: &mut Port,
) {
    assert_eq!(
        backend_runtime::observe(
            expected,
            observed,
            plan,
            BackendMode::OfflineOneShot,
            &DigestFixture,
            port
        ),
        Err(BackendError::InvalidBinding)
    );
}

fn assert_invalid_inputs_refuse(expected: &BackendBinding, plan: &InvocationPlan, port: &mut Port) {
    let mut bad_plan = plan.clone();
    bad_plan.args[1] = "changed".into();
    assert_eq!(
        backend_runtime::observe(
            expected,
            expected,
            &bad_plan,
            BackendMode::OfflineOneShot,
            &DigestFixture,
            port
        ),
        Err(BackendError::InvalidBinding)
    );
    let mut malformed = expected.clone();
    malformed.request.payload.clear();
    assert_eq!(
        backend_runtime::observe(
            &malformed,
            &malformed,
            plan,
            BackendMode::OfflineOneShot,
            &DigestFixture,
            port
        ),
        Err(BackendError::InvalidBinding)
    );
}

fn successful_summary(binding: &BackendBinding) -> BackendSummary {
    BackendSummary {
        binding: binding.clone(),
        result: ResultState::Succeeded,
        sticky_failure: false,
        usable: true,
        terminal: true,
        pending_tools: 0,
        text: "complete".into(),
        cli_session: Some("cli".into()),
    }
}
struct Owner(OwnerObservation);
impl OwnerObservationPort for Owner {
    fn owner_observation(&self) -> OwnerObservation {
        self.0.clone()
    }
}
fn accepted_owner(binding: &BackendBinding, progress_revision: u64, exit: ProcessExit) -> Owner {
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
    let fence = snapshot.fence().unwrap();
    Owner(OwnerObservation {
        snapshot: Some(snapshot),
        capture: Some(CaptureSnapshot {
            binding: CaptureBinding {
                fence,
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
                revision: progress_revision,
                stdout: 0,
                stderr: 0,
                stdout_eof: true,
                stderr_eof: true,
                loss: false,
            },
            exit,
        }),
        poisoned: false,
        pending: false,
    })
}
fn facts(binding: &BackendBinding) -> IndependentFacts {
    IndependentFacts {
        binding: binding.clone(),
        input: InputState::Reconciled,
        inventory: InventoryState::Complete,
        obligations: ObligationState::Satisfied,
        watermark: WatermarkState::Complete,
        containment: ContainmentState::Complete,
        writer: WriterState::Settled,
        claims: ClaimsState::Reconciled,
        execution: ExecutionState::Healthy,
    }
}

#[test]
fn owner_capture_alone_supplies_exit_eof_and_loss_even_below_fence_revision() {
    let binding = binding();
    let result = backend_runtime::project(
        &binding,
        &successful_summary(&binding),
        &facts(&binding),
        &accepted_owner(&binding, 2, ProcessExit::Code(0)),
    );
    assert_eq!(result.workflow, WorkflowState::Succeeded);
}

#[test]
fn missing_cancelled_poisoned_pending_or_lossy_owner_evidence_blocks() {
    let binding = binding();
    let mut base = accepted_owner(&binding, 3, ProcessExit::Code(0)).0;
    for mutate in 0..5 {
        let mut owner = base.clone();
        match mutate {
            0 => owner.capture = None,
            1 => owner.snapshot.as_mut().unwrap().cancelled = true,
            2 => owner.poisoned = true,
            3 => owner.pending = true,
            _ => owner.capture.as_mut().unwrap().progress.loss = true,
        }
        let decision = backend_runtime::project(
            &binding,
            &successful_summary(&binding),
            &facts(&binding),
            &Owner(owner),
        );
        assert_eq!(decision.workflow, WorkflowState::Blocked);
    }
    base.capture.as_mut().unwrap().progress.stdout_eof = false;
    assert_eq!(
        backend_runtime::project(
            &binding,
            &successful_summary(&binding),
            &facts(&binding),
            &Owner(base)
        )
        .workflow,
        WorkflowState::Blocked
    );
}

#[test]
fn unknown_input_inventory_and_summary_from_another_binding_block() {
    let binding = binding();
    let owner = accepted_owner(&binding, 2, ProcessExit::Code(0));
    let mut unknown = facts(&binding);
    unknown.input = InputState::Unknown;
    assert_eq!(
        backend_runtime::project(&binding, &successful_summary(&binding), &unknown, &owner)
            .workflow,
        WorkflowState::Blocked
    );
    let mut unknown = facts(&binding);
    unknown.inventory = InventoryState::Unknown;
    assert_eq!(
        backend_runtime::project(&binding, &successful_summary(&binding), &unknown, &owner)
            .workflow,
        WorkflowState::Blocked
    );
    let mut other = binding.clone();
    other.request.attempt = id("ffffffffffffffffffffffffffffffff");
    let foreign = BackendSummary {
        binding: other,
        result: ResultState::Failed,
        sticky_failure: true,
        usable: true,
        terminal: true,
        pending_tools: 0,
        text: String::new(),
        cli_session: Some("other".into()),
    };
    let decision = backend_runtime::project(&binding, &foreign, &facts(&binding), &owner);
    assert_eq!(decision.result, ResultState::Unknown);
    assert_eq!(decision.workflow, WorkflowState::Blocked);
}

#[test]
fn workflow_success_does_not_complete_release_with_unknown_containment() {
    let binding = binding();
    let owner = accepted_owner(&binding, 2, ProcessExit::Code(0));
    let mut facts = facts(&binding);
    facts.containment = ContainmentState::Unknown;
    let decision =
        backend_runtime::project(&binding, &successful_summary(&binding), &facts, &owner);
    assert_eq!(decision.workflow, WorkflowState::Succeeded);
    assert_eq!(decision.release, ReleaseEvidence::Blocked);
}
