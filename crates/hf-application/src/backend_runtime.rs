//! Fail-closed fixture correlation and read-only owner projection. No launch authority.
use crate::{
    owner::{OwnerPort, OwnerSession},
    owner_policy::evaluate_observation,
    owner_runtime::{DigestPort, StorageCoordinator},
};
use hf_domain::{
    backend::*,
    capture::CaptureSnapshot,
    owner_policy::*,
    ownership::{Phase, Snapshot},
    process::ProcessExit,
};

pub trait BackendObservationPort {
    fn observe(&mut self) -> Result<BackendTrace, BackendError>;
}
pub fn observe(
    expected: &BackendBinding,
    observed: &BackendBinding,
    plan: &InvocationPlan,
    mode: BackendMode,
    digest: &impl DigestPort,
    port: &mut impl BackendObservationPort,
) -> Result<BackendTrace, BackendError> {
    if mode != BackendMode::OfflineOneShot {
        return Err(BackendError::Unsupported);
    }
    if !valid_binding(expected, digest)
        || !valid_binding(observed, digest)
        || expected != observed
        || !valid_plan(plan)
        || plan.executable != expected.logical_executable
        || plan.configuration_digest != expected.configuration_digest
        || configuration_digest(plan, digest) != plan.configuration_digest
    {
        return Err(BackendError::InvalidBinding);
    }
    port.observe()
}
fn valid_binding(b: &BackendBinding, digest: &impl DigestPort) -> bool {
    let r = &b.request;
    !r.payload.is_empty()
        && r.payload.len() <= 16384
        && digest.digest_parts(&[&r.payload]) == r.digest
        && b.authority.issued_attempt == r.attempt
        && b.root.inode != 0
        && b.runtime_version.len() == 7
        && b.runtime_version == "2.1.318"
        && valid_path(&b.logical_executable)
        && valid_path(&b.physical_executable)
        && b.policy.background_idle_seconds.is_finite()
        && b.policy.background_idle_seconds > 0.0
        && b.policy.background_idle_seconds <= 21600.0
        && b.policy.background_requirement != Requirement::Unknown
        && (b.policy.permission_mode == PermissionMode::ToolLevel) == b.policy.tool_level_opt_in
        && if b.authority.role == AuthorityRole::Direct {
            b.authority.checkpoint.is_none() && b.authority.revision.is_none()
        } else {
            b.authority.checkpoint.is_some() && b.authority.revision.is_some()
        }
}
fn valid_path(s: &str) -> bool {
    s.len() > 1
        && s.len() <= 4096
        && s.starts_with('/')
        && !s.chars().any(char::is_control)
        && !s.split('/').any(|p| p == "." || p == "..")
}
#[derive(Clone, Debug)]
pub struct OwnerObservation {
    pub snapshot: Option<Snapshot>,
    pub capture: Option<CaptureSnapshot>,
    pub poisoned: bool,
    pub pending: bool,
}
pub trait OwnerObservationPort {
    fn owner_observation(&self) -> OwnerObservation;
}
impl<P: OwnerPort> OwnerObservationPort for OwnerSession<P> {
    fn owner_observation(&self) -> OwnerObservation {
        OwnerObservation {
            snapshot: self.snapshot().cloned(),
            capture: self.capture().cloned(),
            poisoned: self.is_poisoned(),
            pending: false,
        }
    }
}
impl OwnerObservationPort for StorageCoordinator {
    fn owner_observation(&self) -> OwnerObservation {
        OwnerObservation {
            snapshot: self.snapshot().cloned(),
            capture: self.capture().cloned(),
            poisoned: self.is_poisoned(),
            pending: self.pending().is_some(),
        }
    }
}
/// Only independent fields are accepted; physical exit/EOF/loss come from owner capture.
#[derive(Clone, Debug)]
pub struct IndependentFacts {
    pub binding: BackendBinding,
    pub input: InputState,
    pub inventory: InventoryState,
    pub obligations: ObligationState,
    pub watermark: WatermarkState,
    pub containment: ContainmentState,
    pub writer: WriterState,
    pub claims: ClaimsState,
    pub execution: ExecutionState,
}
pub fn project(
    binding: &BackendBinding,
    summary: &BackendSummary,
    facts: &IndependentFacts,
    owner: &impl OwnerObservationPort,
) -> ObservationDecision {
    let mut value = RuntimeObservation {
        agent_result: ResultState::Unknown,
        result_binding: ResultBinding::Unknown,
        sticky_failure: false,
        turn_open: true,
        requirement: binding.policy.background_requirement,
        obligations: facts.obligations,
        inventory: facts.inventory,
        input: facts.input,
        watermark: facts.watermark,
        containment: facts.containment,
        writer: facts.writer,
        claims: facts.claims,
        execution: facts.execution,
        exit: ExitState::Unobserved,
        stdout: PipeState::Unknown,
        stderr: PipeState::Unknown,
        capture: CaptureState::Unknown,
    };
    value.agent_result = if &summary.binding == binding {
        summary.result
    } else {
        ResultState::Unknown
    };
    value.sticky_failure = &summary.binding == binding && summary.sticky_failure;
    value.result_binding = if summary.usable && &summary.binding == binding {
        ResultBinding::Matched
    } else {
        ResultBinding::Unknown
    };
    value.turn_open = !summary.usable
        || &summary.binding != binding
        || !summary.terminal
        || summary.pending_tools != 0;
    value.requirement = binding.policy.background_requirement;
    value.exit = ExitState::Unobserved;
    value.stdout = PipeState::Unknown;
    value.stderr = PipeState::Unknown;
    value.capture = CaptureState::Unknown;
    if &facts.binding != binding {
        unknown(&mut value);
    }
    apply_owner(binding, &owner.owner_observation(), &mut value);
    evaluate_observation(&value)
}
fn unknown(v: &mut RuntimeObservation) {
    v.input = InputState::Unknown;
    v.inventory = InventoryState::Unknown;
    v.obligations = ObligationState::Unknown;
    v.watermark = WatermarkState::Unknown;
    v.containment = ContainmentState::Unknown;
    v.writer = WriterState::Unknown;
    v.claims = ClaimsState::Unknown;
    v.execution = ExecutionState::Unknown;
}

/// Tagged, counted framing avoids argv/environment boundary ambiguity.
pub fn configuration_digest(
    plan: &InvocationPlan,
    digest: &impl DigestPort,
) -> hf_domain::ownership::Digest {
    let mut bytes = Vec::new();
    for (tag, item) in [
        ("version", "2.1.318"),
        ("executable", plan.executable.as_str()),
        ("cwd", plan.cwd.as_str()),
        ("source", plan.source_profile.as_str()),
    ] {
        frame(&mut bytes, tag);
        frame(&mut bytes, item);
    }
    frame(&mut bytes, "argv");
    bytes.extend_from_slice(&(plan.args.len() as u64).to_be_bytes());
    for arg in &plan.args {
        frame(&mut bytes, arg);
    }
    frame(&mut bytes, "env");
    bytes.extend_from_slice(&(plan.env.len() as u64).to_be_bytes());
    for (key, value) in &plan.env {
        frame(&mut bytes, key);
        frame(&mut bytes, value);
    }
    digest.digest_parts(&[&bytes])
}
fn frame(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn valid_plan(p: &InvocationPlan) -> bool {
    valid_path(&p.executable)
        && valid_path(&p.cwd)
        && p.source_profile.len() <= 1024
        && p.args.len() <= 64
        && p.env.len() <= 32
        && p.args.iter().all(|s| s.len() <= 8192 && !s.contains('\0'))
        && p.env.iter().all(|(k, v)| {
            k.len() <= 128 && v.len() <= 4096 && !k.contains('\0') && !v.contains('\0')
        })
}

fn apply_owner(
    binding: &BackendBinding,
    evidence: &OwnerObservation,
    value: &mut RuntimeObservation,
) {
    if let (Some(s), Some(c)) = (&evidence.snapshot, &evidence.capture)
        && !evidence.poisoned
        && !evidence.pending
        && !s.cancelled
        && s.request == binding.request
        && s.phase == Phase::AcceptanceRecorded
        && s.fence().as_ref() == Some(&c.binding.fence)
        && s.child.as_ref() == Some(&c.binding.child)
        && c.progress.revision <= 128
        && c.progress.stdout <= 65536
        && c.progress.stderr <= 65536
        && c.binding.stdout.inode != 0
        && c.binding.stderr.inode != 0
        && c.binding.stdout != c.binding.stderr
    {
        value.exit = match c.exit {
            ProcessExit::Unobserved => ExitState::Unobserved,
            ProcessExit::Code(0) => ExitState::Zero,
            ProcessExit::Code(_) => ExitState::Nonzero,
            ProcessExit::Signal(_) => ExitState::Signal,
        };
        value.stdout = if c.progress.stdout_eof {
            PipeState::Eof
        } else {
            PipeState::Open
        };
        value.stderr = if c.progress.stderr_eof {
            PipeState::Eof
        } else {
            PipeState::Open
        };
        value.capture = if c.progress.loss {
            CaptureState::KnownLoss
        } else {
            CaptureState::Lossless
        };
    }
}
