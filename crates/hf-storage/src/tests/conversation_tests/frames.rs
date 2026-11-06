//! Stored-frame validation: malformed bytes, foreign roots and rejected values.
use super::fixture::{Fixture, deadline, id, prime};
use crate::{
    Error,
    conversation_codec::{decode, encode},
    tests::fixture_guard,
};
use hf_domain::{
    conversation::{Enrollment, Error as DomainError, Ledger, MAX_FRAME, MAX_TEXT, SendIntent},
    ownership::{Digest, Fence, OwnerClaim, PreparedRequest},
    process::ProcessIdentity,
};

const ROOT: &str = "frame-root";
/// Fixed layout offsets, relative to the request id of the only message.
const WITNESS: usize = 3 * 40 + 8 + 40 + 8;

fn enrollment() -> Enrollment {
    let source = PreparedRequest {
        operation: id(1),
        session: id(2),
        reservation: id(3),
        attempt: id(4),
        generation: id(5),
        digest: Digest::new("a".repeat(64)).unwrap(),
        payload: b"source".to_vec(),
    };
    let process =
        ProcessIdentity::new(41, 99, 23, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap();
    Enrollment {
        conversation: id(8),
        head_task: id(9),
        fence: Fence {
            operation: id(1),
            generation: id(5),
            attempt: id(4),
            owner: OwnerClaim {
                owner: id(6),
                nonce: id(7),
                process: process.clone(),
            },
            revision: 4,
        },
        source,
        child: process,
        context: id(10),
        profile: Digest::new("b".repeat(64)).unwrap(),
        generation: 12,
        turn: id(13),
        ordinary_turns: 22,
        node_attempts: 5,
    }
}

fn ledger() -> Ledger {
    let mut ledger = Ledger::new(enrollment()).unwrap();
    let intent = SendIntent {
        request: id(30),
        expected: ledger.witness(),
        text: "x".into(),
    };
    ledger.send(intent).unwrap();
    ledger
}

struct Layout {
    counters: usize,
    request: usize,
}
fn layout(frame: &[u8]) -> Layout {
    let mut window = Vec::new();
    for n in [12u64, 0, 2, 0, 0, 1, 0] {
        window.extend(n.to_le_bytes());
    }
    let counters = frame.windows(56).position(|w| w == window).unwrap();
    let request = frame
        .windows(32)
        .position(|w| w == id(30).as_str().as_bytes())
        .unwrap();
    Layout { counters, request }
}
fn put(frame: &mut [u8], at: usize, value: u64) {
    frame[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
fn refused(frame: &[u8]) -> Error {
    decode(ROOT, frame).expect_err("frame must be refused")
}

#[test]
fn valid_frame_round_trips_and_layout_assumptions_hold() {
    let value = ledger();
    let frame = encode(ROOT, &value).unwrap();
    assert_eq!(decode(ROOT, &frame).unwrap(), value);
    let at = layout(&frame);
    assert_eq!(frame[at.request + 32 + WITNESS + 8], b'x');
    assert_eq!(
        frame[at.request + 32 + WITNESS + 8 + 1 + 8],
        0,
        "queued tag"
    );
}

#[test]
fn every_truncation_is_refused() {
    let frame = encode(ROOT, &ledger()).unwrap();
    for length in 0..frame.len() {
        assert!(decode(ROOT, &frame[..length]).is_err(), "prefix {length}");
    }
}

#[test]
fn trailing_bytes_foreign_root_and_bad_magic_are_incompatible() {
    let frame = encode(ROOT, &ledger()).unwrap();
    let mut trailing = frame.clone();
    trailing.push(0);
    assert!(matches!(refused(&trailing), Error::Incompatible));
    assert!(matches!(
        decode("other-root", &frame),
        Err(Error::Incompatible)
    ));
    let mut magic = frame;
    magic[0] ^= 0xff;
    assert!(matches!(refused(&magic), Error::Incompatible));
}

#[test]
fn bad_state_tag_and_bad_boolean_are_incompatible() {
    let frame = encode(ROOT, &ledger()).unwrap();
    let at = layout(&frame);
    let state = at.request + 32 + WITNESS + 8 + 1 + 8;
    let mut tag = frame.clone();
    tag[state] = 9;
    assert!(matches!(refused(&tag), Error::Incompatible));
    let mut acknowledged = frame;
    acknowledged[state + 1] = 2;
    assert!(matches!(refused(&acknowledged), Error::Incompatible));
}

#[test]
fn oversize_lengths_and_counts_are_limit_errors() {
    let frame = encode(ROOT, &ledger()).unwrap();
    let at = layout(&frame);
    let text_length = at.request + 32 + WITNESS;
    let message_count = at.request - 16;
    let cases = [
        (5, 33),
        (text_length, MAX_TEXT as u64 + 1),
        (text_length, u64::MAX),
        (message_count, 97),
        (message_count, u64::MAX),
    ];
    for (offset, value) in cases {
        let mut bad = frame.clone();
        put(&mut bad, offset, value);
        assert!(
            matches!(refused(&bad), Error::LimitExceeded),
            "{offset} {value}"
        );
    }
}

#[test]
fn non_canonical_identifiers_are_incompatible() {
    let frame = encode(ROOT, &ledger()).unwrap();
    let at = layout(&frame);
    for (offset, byte) in [(31, b'E'), (31, b'g'), (0, b'-')] {
        let mut bad = frame.clone();
        bad[at.request + offset] = byte;
        assert!(
            matches!(refused(&bad), Error::Incompatible),
            "{offset} {byte}"
        );
    }
}

#[test]
fn patched_counters_fail_validation_without_overflow() {
    let frame = encode(ROOT, &ledger()).unwrap();
    let at = layout(&frame);
    // Field order: generation, epoch, next_sequence, cumulative, unavailable,
    // history_floor, retired.
    let cases = [
        (2, u64::MAX),
        (2, 0),
        (3, 1),
        (3, u64::MAX),
        (4, 1),
        (5, 0),
        (5, 3),
        (6, 1),
        (6, u64::MAX),
        (1, 1),
    ];
    for (field, value) in cases {
        let mut bad = frame.clone();
        put(&mut bad, at.counters + 8 * field, value);
        assert!(
            matches!(refused(&bad), Error::Conversation(DomainError::Invalid)),
            "field {field} value {value}"
        );
    }
}

#[test]
fn encode_validates_before_writing() {
    let mut invalid = ledger();
    invalid.cumulative_issuances = 1;
    assert!(matches!(
        encode(ROOT, &invalid),
        Err(Error::Conversation(DomainError::Invalid))
    ));
}

#[test]
fn stored_text_typed_and_oversized_values_are_refused() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("typed");
    let enrollment = prime(&fixture, 1, false);
    let key = super::fixture::ledger_key(&enrollment);
    let original = fixture.ledger_bytes(&enrollment);
    let update = |sql: &str| {
        fixture.raw_write().execute(sql, [&key]).unwrap();
    };
    update("UPDATE fixture_records SET value='text' WHERE key=?1");
    let store = fixture.reopen();
    let seen = store.conversation_observe(&enrollment.conversation, deadline());
    assert!(matches!(seen, Err(Error::Incompatible)), "{seen:?}");
    drop(store);
    let oversize = format!(
        "UPDATE fixture_records SET value=zeroblob({}) WHERE key=?1",
        MAX_FRAME + 1
    );
    update(&oversize);
    let store = fixture.reopen();
    let seen = store.conversation_observe(&enrollment.conversation, deadline());
    assert!(matches!(seen, Err(Error::LimitExceeded)), "{seen:?}");
    drop(store);
    fixture
        .raw_write()
        .execute(
            "UPDATE fixture_records SET value=?2 WHERE key=?1",
            rusqlite::params![key, original],
        )
        .unwrap();
    let store = fixture.reopen();
    assert_eq!(
        store
            .conversation_observe(&enrollment.conversation, deadline())
            .unwrap()
            .messages
            .len(),
        1
    );
}
