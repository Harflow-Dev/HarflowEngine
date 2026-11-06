//! Offline-only constructor-injected conversation use case. No input transport.
use hf_domain::{
    conversation::{
        BoundaryEvidence, CancelIntent, Claim, ClaimIntent, ClaimOutcome, Enrollment, Ledger,
        Receipt, SendIntent, Witness,
    },
    ownership::Id,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    OfflineFixture,
    Production,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Queue,
    Now,
    Promotion,
    Fresh,
    Resume,
    Automatic,
    Interrupt,
    Workflow,
    Steering,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Error<E> {
    Unsupported,
    Store(E),
    Evidence(E),
    MissingEvidence,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Send(SendIntent),
    Cancel(CancelIntent),
    Claim(ClaimIntent),
    Boundary(BoundaryEvidence),
    Receipt(Receipt),
    Acknowledge(Claim),
    Unknown(Claim),
    Close(Witness),
    RuntimeFault { generation: u64, turn: Id },
}
pub trait ConversationStorePort {
    type Error;
    type Grant;
    type Deadline: Copy;
    fn enroll(
        &self,
        enrollment: &Enrollment,
        deadline: Self::Deadline,
    ) -> Result<Ledger, Self::Error>;
    fn observe(&self, conversation: &Id, deadline: Self::Deadline) -> Result<Ledger, Self::Error>;
    fn apply(
        &self,
        conversation: &Id,
        command: Command,
        deadline: Self::Deadline,
    ) -> Result<ClaimOutcome<Self::Grant>, Self::Error>;
}
/// Positive evidence means terminal, tool, approval and effect settlement have
/// independently passed in this trusted fixture oracle. Absence never grants.
pub trait BoundaryEvidencePort<D, E> {
    fn boundary(&self, expected: &Witness, deadline: D) -> Result<Option<BoundaryEvidence>, E>;
}
pub trait ReceiptEvidencePort<D, E> {
    fn receipt(&self, claim: &Claim, deadline: D) -> Result<Option<Receipt>, E>;
}
pub struct ConversationService<S, B, R> {
    store: S,
    boundary: B,
    receipts: R,
}
impl<S, B, R> ConversationService<S, B, R>
where
    S: ConversationStorePort,
    B: BoundaryEvidencePort<S::Deadline, S::Error>,
    R: ReceiptEvidencePort<S::Deadline, S::Error>,
{
    pub fn new(scope: Scope, store: S, boundary: B, receipts: R) -> Result<Self, Error<S::Error>> {
        if scope != Scope::OfflineFixture {
            return Err(Error::Unsupported);
        }
        Ok(Self {
            store,
            boundary,
            receipts,
        })
    }
    pub fn enroll(
        &self,
        enrollment: &Enrollment,
        deadline: S::Deadline,
    ) -> Result<Ledger, Error<S::Error>> {
        self.store
            .enroll(enrollment, deadline)
            .map_err(Error::Store)
    }
    pub fn observe(
        &self,
        conversation: &Id,
        deadline: S::Deadline,
    ) -> Result<Ledger, Error<S::Error>> {
        self.store
            .observe(conversation, deadline)
            .map_err(Error::Store)
    }
    pub fn send(
        &self,
        route: Route,
        conversation: &Id,
        intent: SendIntent,
        deadline: S::Deadline,
    ) -> Result<Ledger, Error<S::Error>> {
        if route != Route::Queue {
            return Err(Error::Unsupported);
        }
        self.apply(conversation, Command::Send(intent), deadline)
            .map(|o| o.ledger)
    }
    pub fn cancel(
        &self,
        conversation: &Id,
        intent: CancelIntent,
        deadline: S::Deadline,
    ) -> Result<Ledger, Error<S::Error>> {
        self.apply(conversation, Command::Cancel(intent), deadline)
            .map(|o| o.ledger)
    }
    pub fn claim(
        &self,
        route: Route,
        conversation: &Id,
        intent: ClaimIntent,
        deadline: S::Deadline,
    ) -> Result<ClaimOutcome<S::Grant>, Error<S::Error>> {
        if route != Route::Queue {
            return Err(Error::Unsupported);
        }
        // Retained replay is resolved by the transaction before asking for a new
        // boundary. The separate explicit boundary observation never sends input.
        self.apply(conversation, Command::Claim(intent), deadline)
    }
    pub fn observe_boundary(
        &self,
        conversation: &Id,
        expected: &Witness,
        deadline: S::Deadline,
    ) -> Result<Ledger, Error<S::Error>> {
        let evidence = self
            .boundary
            .boundary(expected, deadline)
            .map_err(Error::Evidence)?
            .ok_or(Error::MissingEvidence)?;
        if evidence.expected != *expected {
            return Err(Error::MissingEvidence);
        }
        self.apply(conversation, Command::Boundary(evidence), deadline)
            .map(|o| o.ledger)
    }
    pub fn reconcile(
        &self,
        conversation: &Id,
        claim: &Claim,
        deadline: S::Deadline,
    ) -> Result<Ledger, Error<S::Error>> {
        let evidence = self
            .receipts
            .receipt(claim, deadline)
            .map_err(Error::Evidence)?
            .ok_or(Error::MissingEvidence)?;
        if evidence.claim != *claim {
            return Err(Error::MissingEvidence);
        }
        self.apply(conversation, Command::Receipt(evidence), deadline)
            .map(|o| o.ledger)
    }
    pub fn acknowledge(
        &self,
        conversation: &Id,
        claim: Claim,
        deadline: S::Deadline,
    ) -> Result<Ledger, Error<S::Error>> {
        self.apply(conversation, Command::Acknowledge(claim), deadline)
            .map(|o| o.ledger)
    }
    pub fn unknown(
        &self,
        conversation: &Id,
        claim: Claim,
        deadline: S::Deadline,
    ) -> Result<Ledger, Error<S::Error>> {
        self.apply(conversation, Command::Unknown(claim), deadline)
            .map(|o| o.ledger)
    }
    pub fn close(
        &self,
        conversation: &Id,
        expected: Witness,
        deadline: S::Deadline,
    ) -> Result<Ledger, Error<S::Error>> {
        self.apply(conversation, Command::Close(expected), deadline)
            .map(|o| o.ledger)
    }
    pub fn runtime_fault(
        &self,
        conversation: &Id,
        generation: u64,
        turn: Id,
        deadline: S::Deadline,
    ) -> Result<Ledger, Error<S::Error>> {
        self.apply(
            conversation,
            Command::RuntimeFault { generation, turn },
            deadline,
        )
        .map(|o| o.ledger)
    }
    fn apply(
        &self,
        conversation: &Id,
        command: Command,
        deadline: S::Deadline,
    ) -> Result<ClaimOutcome<S::Grant>, Error<S::Error>> {
        self.store
            .apply(conversation, command, deadline)
            .map_err(Error::Store)
    }
}
