use crate::{fixture_support, task_writer_support};
use hf_application::task_runtime::{
    AcceptanceAdmission, InitialIntentValidator, InvalidIntent, SpawnProjectionFacts,
    SpawnProjectionValidator, TerminalAdmission, TerminalProjectionFacts,
    TerminalProjectionValidator,
};
use hf_domain::{
    ownership::{Digest, Fence, Id, OwnerClaim, PreparedRequest},
    process::ProcessIdentity,
    task_lifecycle::{LifecycleBinding, LifecycleDelta, LifecycleEvidence},
    task_runtime::{AcceptanceReadiness, LaunchIntent, LaunchIntentCreate},
};
use hf_storage::{SpawnPublication, TaskCreate, TaskProcess, TerminalPublication};
use sha2::{Digest as _, Sha256};
use std::cell::{Cell, RefCell};
use std::time::Instant;

pub(super) fn digest(bytes: &[u8]) -> Digest {
    Digest::from_sha256_bytes(Sha256::digest(bytes).into())
}

pub(super) fn id(byte: u8) -> Id {
    Id::new(format!("{byte:032x}")).unwrap()
}

pub(super) fn request(store: &hf_storage::Store, operation: u8) -> PreparedRequest {
    request_with_payload(
        store,
        operation,
        format!("immutable launch preparation {operation}").into_bytes(),
    )
}

pub(super) fn request_with_payload(
    store: &hf_storage::Store,
    operation: u8,
    payload: Vec<u8>,
) -> PreparedRequest {
    PreparedRequest {
        operation: id(operation),
        session: id(operation + 1),
        reservation: id(operation + 2),
        attempt: id(operation + 3),
        generation: Id::new(store.identity().generation.clone()).unwrap(),
        digest: digest(&payload),
        payload,
    }
}

pub(super) fn create(task: &str, prompt: &str) -> LaunchIntentCreate {
    let metadata = b"typed launch metadata fixture".to_vec();
    LaunchIntentCreate {
        task: fixture_support::tid(task),
        metadata_digest: digest(&metadata),
        metadata,
        prompt: prompt.into(),
        prompt_digest: digest(prompt.as_bytes()),
    }
}

pub(super) struct IntentGate {
    pub(super) refuse: bool,
    pub(super) calls: Cell<usize>,
}
impl InitialIntentValidator for IntentGate {
    fn validate(&self, _: &PreparedRequest, _: &LaunchIntentCreate) -> Result<(), InvalidIntent> {
        self.calls.set(self.calls.get() + 1);
        if self.refuse {
            Err(InvalidIntent)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SpawnObservation {
    pub(super) binding: LifecycleBinding,
    pub(super) task: hf_domain::task_read::TaskId,
    pub(super) child: ProcessIdentity,
    pub(super) prompt: String,
}

pub(super) struct SpawnGate {
    pub(super) refuse: bool,
    pub(super) calls: Cell<usize>,
    pub(super) observed: RefCell<Vec<SpawnObservation>>,
}
impl SpawnGate {
    pub(super) fn new(refuse: bool) -> Self {
        Self {
            refuse,
            calls: Cell::new(0),
            observed: RefCell::new(Vec::new()),
        }
    }
}
impl SpawnProjectionValidator for SpawnGate {
    fn validate(
        &self,
        intent: &LaunchIntent,
        facts: &SpawnProjectionFacts<'_>,
        _: &[u8],
        _: &[u8],
        prompt: &str,
    ) -> Result<(), InvalidIntent> {
        self.calls.set(self.calls.get() + 1);
        self.observed.borrow_mut().push(SpawnObservation {
            binding: intent.binding.clone(),
            task: facts.initial.task.clone(),
            child: facts.child.clone(),
            prompt: prompt.into(),
        });
        if self.refuse {
            Err(InvalidIntent)
        } else {
            Ok(())
        }
    }
}

pub(super) struct AcceptanceGate {
    pub(super) refuse: bool,
    pub(super) calls: Cell<usize>,
}
impl AcceptanceAdmission for AcceptanceGate {
    fn admit(
        &self,
        evidence: &LifecycleEvidence,
        readiness: &AcceptanceReadiness,
    ) -> Result<(), InvalidIntent> {
        self.calls.set(self.calls.get() + 1);
        if evidence.ownership.child.as_ref() != Some(&readiness.child)
            || evidence.ownership.phase != hf_domain::ownership::Phase::SpawnObserved
            || readiness.control_endpoint != "/tmp/hf-owner/control.sock"
        {
            return Err(InvalidIntent);
        }
        if self.refuse {
            Err(InvalidIntent)
        } else {
            Ok(())
        }
    }
}

pub(super) struct TerminalAdmissionGate {
    pub(super) refuse: bool,
    pub(super) calls: Cell<usize>,
}
impl TerminalAdmission for TerminalAdmissionGate {
    fn admit(
        &self,
        _: &LifecycleEvidence,
        observation: &hf_domain::task_lifecycle::ExecutionObservation,
    ) -> Result<(), InvalidIntent> {
        self.calls.set(self.calls.get() + 1);
        if self.refuse || !observation.drained || !observation.input_closed {
            Err(InvalidIntent)
        } else {
            Ok(())
        }
    }
}

pub(super) struct TerminalProjectionGate {
    pub(super) refuse: bool,
    pub(super) calls: Cell<usize>,
}
impl TerminalProjectionValidator for TerminalProjectionGate {
    fn validate(
        &self,
        _: &LifecycleEvidence,
        facts: &TerminalProjectionFacts<'_>,
        _: &[u8],
        _: &[u8],
        event_body: &[u8],
        event_seq: u64,
    ) -> Result<(), InvalidIntent> {
        self.calls.set(self.calls.get() + 1);
        if self.refuse
            || !matches!(facts.initial.status, "completed" | "failed" | "cancelled")
            || !facts.output_drained
            || !event_body
                .windows(13)
                .any(|window| window == b"task_finished")
            || event_seq == 0
        {
            Err(InvalidIntent)
        } else {
            Ok(())
        }
    }
}

pub(super) fn child() -> ProcessIdentity {
    ProcessIdentity::new(
        5100,
        1_768_000_000,
        123_456,
        "01234567-89ab-cdef-0123-456789abcdef".into(),
    )
    .unwrap()
}

pub(super) fn owner() -> OwnerClaim {
    OwnerClaim {
        owner: id(50),
        nonce: id(51),
        process: ProcessIdentity::new(
            5000,
            1_768_000_000,
            123_455,
            "01234567-89ab-cdef-0123-456789abcdef".into(),
        )
        .unwrap(),
    }
}

pub(super) fn task_for_spawn(
    task: &str,
    intent: &LaunchIntentCreate,
    process: &ProcessIdentity,
) -> TaskCreate {
    let mut task = task_writer_support::running(task);
    task.facts.session_id = "123e4567-e89b-12d3-a456-426614174000".into();
    task.prompt = intent.prompt.clone();
    task.process = Some(TaskProcess {
        pid: process.pid(),
        start: Some(process.clone()),
    });
    task
}

pub(super) fn spawn_publication<'a>(
    task: &'a TaskCreate,
    child: &'a ProcessIdentity,
    projection: &'a dyn SpawnProjectionValidator,
) -> SpawnPublication<'a> {
    SpawnPublication {
        task,
        child,
        projection,
    }
}

pub(super) fn later(seconds: u64) -> Instant {
    fixture_support::later(seconds)
}

pub(super) fn release_proof(
    world: &fixture_support::World,
    binding: &LifecycleBinding,
) -> Option<String> {
    world
        .db()
        .query_row(
            "SELECT release_proof FROM ownership_records WHERE operation=?1",
            [binding.operation.as_str()],
            |row| row.get(0),
        )
        .unwrap()
}

pub(super) fn claim(store: &hf_storage::Store, binding: &LifecycleBinding) -> (u64, Fence) {
    let claimed = store
        .apply_lifecycle(
            binding,
            None,
            0,
            &LifecycleDelta::OwnerClaimed(owner()),
            later(5),
        )
        .unwrap();
    (claimed.revision, claimed.ownership.fence().unwrap())
}

pub(super) struct SpawnedTask {
    pub(super) intent: LaunchIntentCreate,
    pub(super) evidence: LifecycleEvidence,
    pub(super) child: ProcessIdentity,
    pub(super) fence: Fence,
}

pub(super) fn spawn_task(
    world: &fixture_support::World,
    task_id: &str,
    operation: u8,
    prompt: &str,
) -> SpawnedTask {
    let request = request(&world.store, operation);
    let intent = create(task_id, prompt);
    let prepared = world
        .store
        .prepare_launch_intent(
            &request,
            &intent,
            &IntentGate {
                refuse: false,
                calls: Cell::new(0),
            },
            later(5),
        )
        .unwrap();
    let (revision, fence) = claim(&world.store, &prepared.binding);
    let child = child();
    let task = task_for_spawn(task_id, &intent, &child);
    let spawned = world
        .store
        .publish_launch_spawn(
            &prepared.binding,
            &fence,
            revision,
            &spawn_publication(&task, &child, &SpawnGate::new(false)),
            later(5),
        )
        .unwrap();
    SpawnedTask {
        intent,
        fence: spawned.ownership.fence().unwrap(),
        evidence: spawned,
        child,
    }
}

pub(super) fn readiness(child: &ProcessIdentity) -> AcceptanceReadiness {
    AcceptanceReadiness {
        child: child.clone(),
        control_endpoint: "/tmp/hf-owner/control.sock".into(),
        source_binding: digest(b"raw source binding"),
    }
}

pub(super) fn terminal_publication<'a>(
    task: &'a TaskCreate,
    observation: &'a hf_domain::task_lifecycle::ExecutionObservation,
    event: &'a hf_storage::EventAppend,
    capture: &'a dyn hf_application::task_lifecycle::DurableCaptureAdmission,
    settlement: &'a dyn TerminalAdmission,
    projection: &'a dyn TerminalProjectionValidator,
) -> TerminalPublication<'a> {
    TerminalPublication {
        task,
        observation,
        event,
        capture,
        settlement,
        projection,
    }
}
