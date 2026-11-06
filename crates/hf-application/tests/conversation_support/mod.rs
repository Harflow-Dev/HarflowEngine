#![allow(dead_code)]
use hf_domain::{
    conversation::{
        BoundaryEvidence, CancelIntent, Claim, ClaimIntent, Enrollment, Ledger, Receipt, SendIntent,
    },
    ownership::{Digest, Fence, Id, OwnerClaim, PreparedRequest},
    process::ProcessIdentity,
};

pub fn id(n: u32) -> Id {
    Id::new(format!("{n:032x}")).unwrap()
}
pub fn digest() -> Digest {
    Digest::new("a".repeat(64)).unwrap()
}
pub fn enrollment() -> Enrollment {
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
pub fn fresh() -> Ledger {
    Ledger::new(enrollment()).unwrap()
}
pub fn boundary(ledger: &Ledger, evidence: u32) -> BoundaryEvidence {
    BoundaryEvidence {
        expected: ledger.witness(),
        context: ledger.enrollment.context.clone(),
        profile: ledger.enrollment.profile.clone(),
        evidence: id(evidence),
    }
}
pub fn send(ledger: &Ledger, request: u32, text: &str) -> SendIntent {
    SendIntent {
        request: id(request),
        expected: ledger.witness(),
        text: text.into(),
    }
}
pub fn claim_intent(ledger: &Ledger, request: u32, destination: u32) -> ClaimIntent {
    ClaimIntent {
        request: id(request),
        expected: ledger.witness(),
        destination: id(destination),
    }
}
pub fn cancel_intent(ledger: &Ledger, request: u32, message: u32) -> CancelIntent {
    CancelIntent {
        request: id(request),
        message: id(message),
        expected: ledger.witness(),
    }
}
/// Sends, records a boundary, claims and returns the claim.
pub fn issue(ledger: &mut Ledger, message: u32, claim: u32, destination: u32) -> Claim {
    ledger.send(send(ledger, message, "text")).unwrap();
    if ledger.boundary.is_none() {
        ledger.record_boundary(boundary(ledger, 900)).unwrap();
    }
    ledger
        .claim(claim_intent(ledger, claim, destination))
        .unwrap()
        .unwrap()
}
pub fn consume(ledger: &mut Ledger, claim: Claim, evidence: u32) {
    ledger
        .receipt(Receipt {
            claim,
            evidence: id(evidence),
        })
        .unwrap();
}
/// Sends then cancels `pairs` messages. Request ids start at `base` (two per pair).
pub fn churn(ledger: &mut Ledger, base: u32, pairs: u32) {
    for n in 0..pairs {
        let message = base + 2 * n;
        ledger.send(send(ledger, message, "c")).unwrap();
        ledger
            .cancel(cancel_intent(ledger, message + 1, message))
            .unwrap();
    }
}
