use hf_application::conversation::{
    BoundaryEvidencePort, Command, ConversationService, ConversationStorePort, ReceiptEvidencePort,
    Route, Scope,
};
use hf_domain::{
    conversation::{
        BoundaryEvidence, CancelIntent, ClaimIntent, Enrollment, Error, Ledger, MessageState,
        Receipt, SendIntent,
    },
    ownership::{Digest, Fence, Id, OwnerClaim, PreparedRequest},
    process::ProcessIdentity,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn id(n: u8) -> Id {
    Id::new(format!("{n:032x}")).unwrap()
}
fn digest() -> Digest {
    Digest::new("a".repeat(64)).unwrap()
}
fn enrollment() -> Enrollment {
    let source = PreparedRequest {
        operation: id(1),
        session: id(2),
        reservation: id(3),
        attempt: id(4),
        generation: id(5),
        digest: digest(),
        payload: b"source request".to_vec(),
    };
    let process =
        ProcessIdentity::new(41, 99, 23, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap();
    let owner = OwnerClaim {
        owner: id(6),
        nonce: id(7),
        process: process.clone(),
    };
    Enrollment {
        conversation: id(8),
        head_task: id(9),
        fence: Fence {
            operation: source.operation.clone(),
            generation: source.generation.clone(),
            attempt: source.attempt.clone(),
            owner,
            revision: 4,
        },
        source,
        child: process,
        context: id(10),
        profile: digest(),
        generation: 12,
        turn: id(13),
        ordinary_turns: 22,
        node_attempts: 5,
    }
}
fn boundary(ledger: &Ledger) -> BoundaryEvidence {
    BoundaryEvidence {
        expected: ledger.witness(),
        context: ledger.enrollment.context.clone(),
        profile: ledger.enrollment.profile.clone(),
        evidence: id(11),
    }
}
fn send(ledger: &Ledger, request: u8, text: &str) -> SendIntent {
    SendIntent {
        request: id(request),
        expected: ledger.witness(),
        text: text.into(),
    }
}
fn claim(ledger: &Ledger, request: u8) -> ClaimIntent {
    ClaimIntent {
        request: id(request),
        expected: ledger.witness(),
        destination: id(request.wrapping_add(40)),
    }
}

#[test]
fn fifo_claims_one_per_positive_boundary_and_ack_is_not_consumption() {
    let mut ledger = Ledger::new(enrollment()).unwrap();
    ledger.send(send(&ledger, 20, "  exact\ntext\t")).unwrap();
    ledger.send(send(&ledger, 21, "second")).unwrap();
    assert_eq!(ledger.messages[0].intent.text, "  exact\ntext\t");
    assert!(matches!(
        ledger.claim(claim(&ledger, 30)),
        Err(Error::Blocked)
    ));
    ledger.record_boundary(boundary(&ledger)).unwrap();
    let first = ledger.claim(claim(&ledger, 30)).unwrap().unwrap();
    assert_eq!(first.message, id(20));
    assert_eq!(first.turn, id(70));
    assert_eq!(ledger.messages[0].state, MessageState::Claimed);
    assert_eq!(ledger.cumulative_issuances, 1);
    assert_eq!(ledger.claim(claim(&ledger, 31)), Err(Error::Blocked));
    ledger.acknowledge(&first).unwrap();
    assert!(ledger.messages[0].acknowledged);
    assert_eq!(ledger.messages[0].state, MessageState::Claimed);
    assert_eq!(ledger.cumulative_issuances, 1);
}

#[test]
fn unknown_claim_blocks_issuance_but_allows_bounded_queue_admission() {
    let mut ledger = Ledger::new(enrollment()).unwrap();
    ledger.send(send(&ledger, 20, "one")).unwrap();
    ledger.record_boundary(boundary(&ledger)).unwrap();
    let issued = ledger.claim(claim(&ledger, 30)).unwrap().unwrap();
    ledger.unknown(&issued).unwrap();
    assert_eq!(ledger.messages[0].state, MessageState::Unknown);
    ledger.send(send(&ledger, 21, "two")).unwrap();
    assert_eq!(ledger.claim(claim(&ledger, 31)), Err(Error::Blocked));
    assert_eq!(ledger.unavailable_usage, 1);
}

#[test]
fn only_exact_correlated_receipt_consumes_and_replay_does_not_increment_usage() {
    let mut ledger = Ledger::new(enrollment()).unwrap();
    ledger.send(send(&ledger, 20, "one")).unwrap();
    ledger.record_boundary(boundary(&ledger)).unwrap();
    let issued = ledger.claim(claim(&ledger, 30)).unwrap().unwrap();
    ledger.unknown(&issued).unwrap();
    let receipt = Receipt {
        claim: issued.clone(),
        evidence: id(13),
    };
    ledger.receipt(receipt.clone()).unwrap();
    ledger.receipt(receipt).unwrap();
    assert_eq!(ledger.messages[0].state, MessageState::Consumed);
    assert_eq!(ledger.cumulative_issuances, 1);
    assert_eq!(ledger.messages[0].receipt.as_ref().unwrap().claim, issued);
}

#[test]
fn cancellation_is_queued_only_and_has_separate_idempotency_identity() {
    let mut ledger = Ledger::new(enrollment()).unwrap();
    ledger.send(send(&ledger, 20, "one")).unwrap();
    let cancel = CancelIntent {
        request: id(22),
        message: id(20),
        expected: ledger.witness(),
    };
    ledger.cancel(cancel.clone()).unwrap();
    ledger.cancel(cancel).unwrap();
    assert_eq!(ledger.messages[0].state, MessageState::Cancelled);
    ledger.record_boundary(boundary(&ledger)).unwrap();
    assert_eq!(ledger.close(&ledger.witness()), Ok(()));
}

#[test]
fn rolling_history_allows_more_than_sixty_four_sequential_messages_without_changing_ordinary_counts()
 {
    let mut ledger = Ledger::new(enrollment()).unwrap();
    for n in 0..100u8 {
        let request = id(n.wrapping_add(30));
        ledger
            .send(SendIntent {
                request: request.clone(),
                expected: ledger.witness(),
                text: format!("message-{n}"),
            })
            .unwrap();
        ledger.record_boundary(boundary(&ledger)).unwrap();
        let issued = ledger
            .claim(ClaimIntent {
                request: id(n.wrapping_add(140)),
                expected: ledger.witness(),
                destination: id(n.wrapping_add(180)),
            })
            .unwrap()
            .unwrap();
        assert_eq!(issued.message, request);
        ledger
            .receipt(Receipt {
                claim: issued,
                evidence: id(n.wrapping_add(220)),
            })
            .unwrap();
        ledger.record_boundary(boundary(&ledger)).unwrap();
    }
    assert_eq!(ledger.cumulative_issuances, 100);
    assert_eq!(ledger.unavailable_usage, 100);
    assert!(ledger.retired_messages > 0);
    assert!(ledger.history_gaps);
    assert_eq!(
        (
            ledger.enrollment.ordinary_turns,
            ledger.enrollment.node_attempts
        ),
        (22, 5)
    );
}

#[test]
fn close_requires_boundary_and_no_outstanding_work() {
    let mut ledger = Ledger::new(enrollment()).unwrap();
    assert_eq!(ledger.close(&ledger.witness()), Err(Error::Blocked));
    ledger.send(send(&ledger, 20, "pending")).unwrap();
    ledger.record_boundary(boundary(&ledger)).unwrap();
    assert_eq!(ledger.close(&ledger.witness()), Err(Error::Blocked));
}

#[test]
fn retained_send_replay_precedes_stale_witness_but_changed_payload_conflicts() {
    let mut ledger = Ledger::new(enrollment()).unwrap();
    let original = send(&ledger, 20, "same");
    ledger.send(original.clone()).unwrap();
    let mut changed = original.clone();
    changed.text.push('!');
    assert_eq!(ledger.send(changed), Err(Error::Conflict));
    ledger.record_boundary(boundary(&ledger)).unwrap();
    assert_eq!(ledger.send(original), Ok(()));
}

struct MemoryStore {
    ledger: Mutex<Option<Ledger>>,
    calls: Arc<AtomicUsize>,
}
impl ConversationStorePort for MemoryStore {
    type Error = ();
    type Deadline = ();
    type Grant = ();
    fn enroll(&self, e: &Enrollment, _: ()) -> Result<Ledger, ()> {
        *self.ledger.lock().unwrap() = Some(Ledger::new(e.clone()).unwrap());
        Ok(self.ledger.lock().unwrap().clone().unwrap())
    }
    fn observe(&self, _: &Id, _: ()) -> Result<Ledger, ()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.ledger.lock().unwrap().clone().unwrap())
    }
    fn apply(
        &self,
        _: &Id,
        command: Command,
        _: (),
    ) -> Result<hf_domain::conversation::ClaimOutcome<Self::Grant>, ()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut slot = self.ledger.lock().unwrap();
        let ledger = slot.as_mut().unwrap();
        match command {
            Command::Send(v) => {
                ledger.send(v).unwrap();
            }
            Command::Claim(v) => {
                ledger.claim(v).unwrap();
            }
            Command::Boundary(v) => {
                ledger.record_boundary(v).unwrap();
            }
            _ => {}
        }
        Ok(hf_domain::conversation::ClaimOutcome {
            ledger: ledger.clone(),
            grant: None,
        })
    }
}
struct NoBoundary;
impl BoundaryEvidencePort<(), ()> for NoBoundary {
    fn boundary(
        &self,
        _: &hf_domain::conversation::Witness,
        _: (),
    ) -> Result<Option<BoundaryEvidence>, ()> {
        Ok(None)
    }
}
impl ReceiptEvidencePort<(), ()> for NoBoundary {
    fn receipt(&self, _: &hf_domain::conversation::Claim, _: ()) -> Result<Option<Receipt>, ()> {
        Ok(None)
    }
}

#[test]
fn unsupported_routes_are_refused_before_store_or_evidence_io() {
    let calls = Arc::new(AtomicUsize::new(0));
    let service = ConversationService::new(
        Scope::OfflineFixture,
        MemoryStore {
            ledger: Mutex::new(None),
            calls: calls.clone(),
        },
        NoBoundary,
        NoBoundary,
    )
    .unwrap();
    let conversation = id(8);
    assert!(matches!(
        service.send(
            Route::Now,
            &conversation,
            SendIntent {
                request: id(20),
                expected: Ledger::new(enrollment()).unwrap().witness(),
                text: "x".into()
            },
            ()
        ),
        Err(hf_application::conversation::Error::Unsupported)
    ));
    assert!(matches!(
        service.claim(
            Route::Automatic,
            &conversation,
            claim(&Ledger::new(enrollment()).unwrap(), 30),
            ()
        ),
        Err(hf_application::conversation::Error::Unsupported)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
