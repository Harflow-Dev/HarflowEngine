#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Drives the domain ledger and compares a normalized projection with the
//! vendored canonical vectors. Reads only `tests/fixtures/contracts-v1/`.
use hf_domain::{
    conversation::{
        BoundaryEvidence, CancelIntent, ClaimIntent, Enrollment, Error, Ledger, MessageState,
        Receipt, SendIntent,
    },
    ownership::{Digest, Fence, Id, OwnerClaim, PreparedRequest},
    process::ProcessIdentity,
};
use serde_json::{Map, Value};

const FIRST: &str = "Keep  two spaces.\nFollow-up prose stays verbatim.";
const SECOND: &str = "Second FIFO message.";

fn id(n: u32) -> Id {
    Id::new(format!("{n:032x}")).unwrap()
}
fn vector(name: &str) -> Value {
    let path = format!(
        "{}/../../tests/fixtures/contracts-v1/ctl-conversation-{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}
fn undash(value: &Value) -> Value {
    Value::from(value.as_str().unwrap().replace('-', ""))
}

fn ledger() -> Ledger {
    let process =
        ProcessIdentity::new(41, 99, 23, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap();
    let source = PreparedRequest {
        operation: id(0xf1),
        session: id(0xf2),
        reservation: id(0xf3),
        attempt: id(0xf4),
        generation: id(0xf5),
        digest: Digest::new("a".repeat(64)).unwrap(),
        payload: b"source".to_vec(),
    };
    Ledger::new(Enrollment {
        conversation: id(1),
        head_task: id(0xa),
        fence: Fence {
            operation: source.operation.clone(),
            generation: source.generation.clone(),
            attempt: source.attempt.clone(),
            owner: OwnerClaim {
                owner: id(0xf6),
                nonce: id(0xf7),
                process: process.clone(),
            },
            revision: 4,
        },
        source,
        child: process,
        context: id(0xc0),
        profile: Digest::new("b".repeat(64)).unwrap(),
        generation: 1,
        turn: id(0xb),
        ordinary_turns: 0,
        node_attempts: 0,
    })
    .unwrap()
}
fn send(l: &mut Ledger, request: u32, text: &str) {
    let intent = SendIntent {
        request: id(request),
        expected: l.witness(),
        text: text.into(),
    };
    l.send(intent).unwrap();
}
fn fifo() -> Ledger {
    let mut l = ledger();
    send(&mut l, 0x65, FIRST);
    send(&mut l, 0x66, SECOND);
    l
}
fn boundary(l: &mut Ledger) {
    let evidence = BoundaryEvidence {
        expected: l.witness(),
        context: l.enrollment.context.clone(),
        profile: l.enrollment.profile.clone(),
        evidence: id(0xe0),
    };
    l.record_boundary(evidence).unwrap();
}
fn cancel_intent(l: &Ledger) -> CancelIntent {
    CancelIntent {
        request: id(0xc9),
        message: id(0x65),
        expected: l.witness(),
    }
}
fn status(state: MessageState) -> &'static str {
    match state {
        MessageState::Queued => "queued",
        MessageState::Claimed => "claimed",
        MessageState::Consumed => "consumed",
        MessageState::Unknown => "unknown",
        MessageState::Cancelled => "cancelled",
    }
}

fn witness_map(parts: [(&str, Value); 5]) -> Map<String, Value> {
    let mut witness = Map::new();
    for (k, v) in parts {
        witness.insert(k.into(), v);
    }
    witness
}

fn base(
    witness: Map<String, Value>,
    closed: Value,
    cumulative: Value,
    messages: Vec<Value>,
) -> Value {
    let mut out = Map::new();
    out.insert("witness".into(), Value::Object(witness));
    out.insert("closed".into(), closed);
    out.insert("cumulative_issuances".into(), cumulative);
    out.insert("messages".into(), Value::Array(messages));
    Value::Object(out)
}

fn message(fields: [(&str, Value); 8], cancellation: Option<Value>) -> Value {
    let mut out = Map::new();
    for (k, v) in fields {
        out.insert(k.into(), v);
    }
    if let Some(v) = cancellation {
        out.insert("cancellation_id".into(), v);
    }
    Value::Object(out)
}

fn project(l: &Ledger) -> Value {
    let w = l.witness();
    let messages = l
        .messages
        .iter()
        .map(|m| {
            let source = &m.intent.expected;
            message(
                [
                    ("id", m.intent.request.as_str().into()),
                    ("sequence", m.sequence.into()),
                    ("status", status(m.state).into()),
                    ("text", m.intent.text.as_str().into()),
                    ("source_generation", source.generation.into()),
                    ("source_turn", source.turn.as_str().into()),
                    ("source_epoch", source.ledger_epoch.into()),
                    ("delivery", "queue".into()),
                ],
                m.cancellation.as_ref().map(|c| c.request.as_str().into()),
            )
        })
        .collect();
    let witness = witness_map([
        ("conversation", w.conversation.as_str().into()),
        ("head_task", w.head_task.as_str().into()),
        ("generation", w.generation.into()),
        ("turn", w.turn.as_str().into()),
        ("ledger_epoch", w.ledger_epoch.into()),
    ]);
    base(
        witness,
        l.closed.into(),
        l.cumulative_issuances.into(),
        messages,
    )
}

fn from_vector(v: &Value) -> Value {
    let r = &v["result"];
    let e = &r["expected"];
    let messages = r["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            message(
                [
                    ("id", undash(&m["request_id"])),
                    ("sequence", m["sequence"].clone()),
                    ("status", m["status"].clone()),
                    ("text", m["text"].clone()),
                    ("source_generation", m["source_generation"].clone()),
                    ("source_turn", undash(&m["source_turn_id"])),
                    ("source_epoch", m["source_epoch"].clone()),
                    ("delivery", m["delivery"].clone()),
                ],
                m.get("cancellation_request_id").map(undash),
            )
        })
        .collect();
    let witness = witness_map([
        ("conversation", undash(&e["conversation_id"])),
        ("head_task", undash(&e["head_task_id"])),
        ("generation", e["generation"].clone()),
        ("turn", undash(&e["turn_id"])),
        ("ledger_epoch", e["ledger_epoch"].clone()),
    ]);
    base(
        witness,
        r["closed"].clone(),
        r["cumulative_issuances"].clone(),
        messages,
    )
}

fn assert_matches_vector(name: &str, l: &Ledger) {
    assert_eq!(project(l), from_vector(&vector(name)), "{name}");
}

#[test]
fn empty_and_closed_ledgers_match() {
    assert_matches_vector("empty", &ledger());
    let mut closed = ledger();
    boundary(&mut closed);
    closed.close(&closed.witness()).unwrap();
    assert_matches_vector("closed", &closed);
}

#[test]
fn queue_fifo_and_replay_match() {
    let mut l = fifo();
    assert_matches_vector("queue-fifo", &l);
    let before = l.clone();
    send(&mut l, 0x65, FIRST);
    send(&mut l, 0x66, SECOND);
    assert_eq!(l, before);
    assert_matches_vector("queue-replayed", &l);
}

#[test]
fn cancellation_and_replay_match() {
    let mut l = fifo();
    l.cancel(cancel_intent(&l)).unwrap();
    assert_matches_vector("cancelled", &l);
    let before = l.clone();
    l.cancel(cancel_intent(&l)).unwrap();
    assert_eq!(l, before);
    assert_matches_vector("cancel-replayed", &l);
}

/// The canonical consumed vectors use Now delivery, so only the status
/// vocabulary and the destination generation and turn are compared.
fn claimed() -> Ledger {
    let mut l = fifo();
    boundary(&mut l);
    let intent = ClaimIntent {
        request: id(0x67),
        expected: l.witness(),
        destination: id(0xc),
    };
    let claim = l.claim(intent).unwrap().unwrap();
    assert_eq!(claim.message, id(0x65));
    l
}
fn third(name: &str) -> (Value, Value) {
    let v = vector(name);
    (v["result"]["messages"][2].clone(), v["result"].clone())
}
fn assert_destination(l: &Ledger, name: &str, expected_status: &str) {
    let (m, result) = third(name);
    assert_eq!(m["destination"]["generation"], Value::from(l.generation));
    assert_eq!(
        undash(&m["destination"]["turn_id"]),
        Value::from(l.turn.as_str())
    );
    assert_eq!(result["expected"]["generation"], Value::from(l.generation));
    assert_eq!(
        result["cumulative_issuances"],
        Value::from(l.cumulative_issuances)
    );
    assert_eq!(m["status"], Value::from(expected_status));
    assert_eq!(status(l.messages[0].state), expected_status);
}

#[test]
fn now_vectors_agree_on_status_vocabulary_and_destination() {
    let mut l = claimed();
    l.unknown(&l.messages[0].claim.clone().unwrap()).unwrap();
    assert_destination(&l, "delivery-unknown", "unknown");
    let claim = l.messages[0].claim.clone().unwrap();
    let receipt = Receipt {
        claim,
        evidence: id(0xe1),
    };
    l.receipt(receipt).unwrap();
    assert_destination(&l, "unknown-reconciled-synthetic", "consumed");
    let mut direct = claimed();
    let claim = direct.messages[0].claim.clone().unwrap();
    direct
        .receipt(Receipt {
            claim,
            evidence: id(0xe2),
        })
        .unwrap();
    assert_destination(&direct, "consumed-synthetic", "consumed");
}

/// Test-local vocabulary only: the refusal vectors carry a code and no trigger
/// scenario, so this does not show that the triggers match the canonical engine.
fn code(error: Error) -> &'static str {
    match error {
        Error::AlreadyClaimed => "already_claimed",
        Error::Capacity => "queue_full",
        Error::Expired => "request_expired",
        Error::Stale => "target_stale",
        Error::Conflict => "request_conflict",
        Error::Fault => "conversation_unknown",
        _ => "unmapped",
    }
}
fn assert_code(name: &str, error: Error) {
    let v = vector(name);
    assert_eq!(v["error"]["code"], Value::from(code(error)), "{name}");
    assert_eq!(v["error"]["message"], v["error"]["code"], "{name}");
}

#[test]
fn refusal_codes_map_domain_errors() {
    let mut l = claimed();
    l.unknown(&l.messages[0].claim.clone().unwrap()).unwrap();
    let issued = cancel_intent(&l);
    assert_code("refusal-cancel-issued", l.cancel(issued).unwrap_err());

    let mut full = ledger();
    for n in 0..32 {
        send(&mut full, 0x100 + n, "q");
    }
    let intent = SendIntent {
        request: id(0x200),
        expected: full.witness(),
        text: "q".into(),
    };
    assert_code("refusal-queue-full", full.send(intent).unwrap_err());

    let mut l = fifo();
    let mut intent = SendIntent {
        request: id(0x201),
        expected: l.witness(),
        text: "q".into(),
    };
    intent.expected.ledger_epoch = 1;
    assert_code(
        "refusal-request-expired",
        l.send(intent.clone()).unwrap_err(),
    );
    intent.expected.ledger_epoch = 0;
    intent.expected.turn = id(0x999);
    assert_code("refusal-target-stale", l.send(intent).unwrap_err());

    let changed = SendIntent {
        request: id(0x65),
        expected: l.witness(),
        text: "different".into(),
    };
    assert_code("refusal-request-conflict", l.send(changed).unwrap_err());

    // conversation_unknown is a runtime fault; a missing cancel target is
    // intentionally not mapped to it.
    l.fault(l.generation, &l.turn.clone()).unwrap();
    let after_fault = SendIntent {
        request: id(0x202),
        expected: l.witness(),
        text: "q".into(),
    };
    assert_code(
        "refusal-conversation-unknown",
        l.send(after_fault).unwrap_err(),
    );
}
