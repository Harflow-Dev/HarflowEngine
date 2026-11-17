use crate::fixture_support::{self, later};
use hf_application::task_lifecycle::{
    CaptureAdmissionRefused, CaptureProgress, DurableCaptureAdmission, DurableCaptureRequest,
};
use hf_application::task_lifecycle::{
    InitialProjectionFacts, InitialProjectionValidator, InvalidInitialProjection,
};
use hf_domain::{
    ownership::{Digest, Id, OwnerClaim, PreparedRequest},
    process::ProcessIdentity,
    task_lifecycle::{LifecycleBinding, LifecycleDelta},
};
use sha2::{Digest as _, Sha256};
use std::cell::RefCell;

pub(super) struct AcceptProjection;
impl InitialProjectionValidator for AcceptProjection {
    fn validate(
        &self,
        _: &InitialProjectionFacts<'_>,
        _: &[u8],
        _: &[u8],
    ) -> Result<(), InvalidInitialProjection> {
        Ok(())
    }
}
pub(super) static ACCEPT: AcceptProjection = AcceptProjection;

pub(super) struct RejectProjection;
impl InitialProjectionValidator for RejectProjection {
    fn validate(
        &self,
        _: &InitialProjectionFacts<'_>,
        _: &[u8],
        _: &[u8],
    ) -> Result<(), InvalidInitialProjection> {
        Err(InvalidInitialProjection)
    }
}
pub(super) static REJECT: RejectProjection = RejectProjection;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CaptureRequestSnapshot {
    pub(super) binding: LifecycleBinding,
    pub(super) child: ProcessIdentity,
    pub(super) previous: CaptureProgress,
    pub(super) next: CaptureProgress,
}

pub(super) struct CaptureGate {
    refuse: bool,
    pub(super) requests: RefCell<Vec<CaptureRequestSnapshot>>,
}
impl CaptureGate {
    pub(super) fn new(refuse: bool) -> Self {
        Self {
            refuse,
            requests: RefCell::new(Vec::new()),
        }
    }
}
impl DurableCaptureAdmission for CaptureGate {
    fn admit(&self, request: &DurableCaptureRequest<'_>) -> Result<(), CaptureAdmissionRefused> {
        self.requests.borrow_mut().push(CaptureRequestSnapshot {
            binding: request.binding.clone(),
            child: request.child.clone(),
            previous: request.previous,
            next: request.next,
        });
        if self.refuse {
            Err(CaptureAdmissionRefused)
        } else {
            Ok(())
        }
    }
}

pub(super) fn id(byte: u8) -> Id {
    Id::new(format!("{byte:032x}")).unwrap()
}

pub(super) fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(
        pid,
        1_768_000_000,
        123_456,
        "01234567-89ab-cdef-0123-456789abcdef".into(),
    )
    .unwrap()
}

pub(super) fn request(store: &hf_storage::Store, operation: u8) -> PreparedRequest {
    let payload = format!("lifecycle fixture {operation}").into_bytes();
    PreparedRequest {
        operation: id(operation),
        session: id(operation + 1),
        reservation: id(operation + 2),
        attempt: id(operation + 3),
        generation: id_from_generation(&store.identity().generation),
        digest: Digest::new(format!("{:x}", Sha256::digest(&payload))).unwrap(),
        payload,
    }
}

pub(super) fn id_from_generation(value: &str) -> Id {
    Id::new(value.to_owned()).unwrap()
}

pub(super) fn owner(tag: u8) -> OwnerClaim {
    OwnerClaim {
        owner: id(tag),
        nonce: id(tag + 1),
        process: process(4000 + u32::from(tag)),
    }
}

pub(super) fn claim_child(
    world: &fixture_support::World,
    binding: &LifecycleBinding,
) -> hf_domain::task_lifecycle::LifecycleEvidence {
    let claimed = world
        .store
        .apply_lifecycle(
            binding,
            None,
            0,
            &LifecycleDelta::OwnerClaimed(owner(20)),
            later(5),
        )
        .unwrap();
    world
        .store
        .apply_lifecycle(
            binding,
            claimed.ownership.fence().as_ref(),
            1,
            &LifecycleDelta::ChildRecorded(process(5000)),
            later(5),
        )
        .unwrap()
}

pub(super) fn assert_capture_request(
    capture: &CaptureGate,
    binding: &LifecycleBinding,
    child: &ProcessIdentity,
    next: CaptureProgress,
) {
    assert_eq!(
        capture.requests.borrow().as_slice(),
        &[CaptureRequestSnapshot {
            binding: binding.clone(),
            child: child.clone(),
            previous: CaptureProgress {
                stdout_end: 0,
                stderr_end: 0,
                known_loss: false,
            },
            next,
        }]
    );
}
