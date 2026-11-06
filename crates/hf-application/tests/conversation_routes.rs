#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod conversation_support;
use conversation_support::*;
use hf_application::conversation::{
    BoundaryEvidencePort, Command, ConversationService, ConversationStorePort, Error,
    ReceiptEvidencePort, Route, Scope,
};
use hf_domain::{
    conversation::{BoundaryEvidence, Claim, ClaimOutcome, Enrollment, Ledger, Receipt, Witness},
    ownership::Id,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Default)]
struct Calls {
    count: Arc<AtomicUsize>,
    commands: Arc<Mutex<Vec<Command>>>,
}
struct Store(Calls);
impl ConversationStorePort for Store {
    type Error = ();
    type Deadline = ();
    type Grant = ();
    fn enroll(&self, e: &Enrollment, _: ()) -> Result<Ledger, ()> {
        self.0.count.fetch_add(1, Ordering::SeqCst);
        Ok(Ledger::new(e.clone()).unwrap())
    }
    fn observe(&self, _: &Id, _: ()) -> Result<Ledger, ()> {
        self.0.count.fetch_add(1, Ordering::SeqCst);
        Ok(fresh())
    }
    fn apply(&self, _: &Id, command: Command, _: ()) -> Result<ClaimOutcome<()>, ()> {
        self.0.count.fetch_add(1, Ordering::SeqCst);
        self.0.commands.lock().unwrap().push(command);
        Ok(ClaimOutcome {
            ledger: fresh(),
            grant: None,
        })
    }
}
struct Evidence(Calls);
impl BoundaryEvidencePort<(), ()> for Evidence {
    fn boundary(&self, _: &Witness, _: ()) -> Result<Option<BoundaryEvidence>, ()> {
        self.0.count.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }
}
impl ReceiptEvidencePort<(), ()> for Evidence {
    fn receipt(&self, _: &Claim, _: ()) -> Result<Option<Receipt>, ()> {
        self.0.count.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }
}

fn service(calls: &Calls) -> ConversationService<Store, Evidence, Evidence> {
    ConversationService::new(
        Scope::OfflineFixture,
        Store(calls.clone()),
        Evidence(calls.clone()),
        Evidence(calls.clone()),
    )
    .unwrap()
}

const REFUSED: [Route; 8] = [
    Route::Now,
    Route::Promotion,
    Route::Fresh,
    Route::Resume,
    Route::Automatic,
    Route::Interrupt,
    Route::Workflow,
    Route::Steering,
];

#[test]
fn every_route_except_queue_is_refused_before_any_port_call() {
    let calls = Calls::default();
    let service = service(&calls);
    let ledger = fresh();
    for route in REFUSED {
        let sent = service.send(route, &id(8), send(&ledger, 30, "x"), ());
        assert!(matches!(sent, Err(Error::Unsupported)), "send {route:?}");
        let claimed = service.claim(route, &id(8), claim_intent(&ledger, 31, 80), ());
        assert!(
            matches!(claimed, Err(Error::Unsupported)),
            "claim {route:?}"
        );
    }
    assert_eq!(calls.count.load(Ordering::SeqCst), 0);
}

#[test]
fn queue_route_reaches_the_store_once_per_call() {
    let calls = Calls::default();
    let service = service(&calls);
    let ledger = fresh();
    service
        .send(Route::Queue, &id(8), send(&ledger, 30, "x"), ())
        .unwrap();
    service
        .claim(Route::Queue, &id(8), claim_intent(&ledger, 31, 80), ())
        .unwrap();
    assert_eq!(calls.count.load(Ordering::SeqCst), 2);
}

#[test]
fn production_scope_is_refused_at_construction() {
    let calls = Calls::default();
    let built = ConversationService::new(
        Scope::Production,
        Store(calls.clone()),
        Evidence(calls.clone()),
        Evidence(calls.clone()),
    );
    assert!(matches!(built, Err(Error::Unsupported)));
    assert_eq!(calls.count.load(Ordering::SeqCst), 0);
}

#[test]
fn runtime_fault_forwards_generation_and_turn() {
    let calls = Calls::default();
    let service = service(&calls);
    service.runtime_fault(&id(8), 17, id(77), ()).unwrap();
    let commands = calls.commands.lock().unwrap();
    assert_eq!(commands.len(), 1);
    assert_eq!(
        commands[0],
        Command::RuntimeFault {
            generation: 17,
            turn: id(77)
        }
    );
}
