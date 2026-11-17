#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_application::owner_runtime::DigestPort;
use hf_domain::{
    owner_runtime::{
        AckDisposition, CommandIdentity, StorageAck, StorageCommand, StorageRefusal, Stream,
        TrustedDrainEvidence,
    },
    ownership::Digest,
};
use hf_interfaces::owner_storage_codec::{CodecError, Message, decode, encode};

struct TestDigest;
impl DigestPort for TestDigest {
    fn digest_parts(&self, parts: &[&[u8]]) -> Digest {
        let mut output = [0_u8; 32];
        let mut index = 0_usize;
        for part in parts {
            for byte in *part {
                output[index] = output[index].wrapping_mul(31).wrapping_add(*byte);
                index = (index + 13) % output.len();
            }
        }
        Digest::from_sha256_bytes(output)
    }
}

fn digest(c: char) -> Digest {
    Digest::new(c.to_string().repeat(64)).unwrap()
}
fn sample(payload: Vec<u8>) -> Message {
    let drain = payload.is_empty().then_some(TrustedDrainEvidence {
        reader_joined: true,
        accepted_data_reconciled: true,
        input_reconciled: true,
        stdout_eof: true,
        stderr_eof: true,
    });
    Message::Command(StorageCommand::Append {
        stream: Stream::Stdout,
        bytes: payload,
        drain,
    })
}

fn refusal_message(cause: StorageRefusal) -> Message {
    Message::Ack(Box::new(StorageAck {
        identity: CommandIdentity {
            sequence: 1,
            binding_digest: digest('a'),
            command_digest: digest('b'),
        },
        disposition: AckDisposition::Refused,
        refusal: Some(cause),
        revision: None,
        snapshot: None,
        capture: None,
        raw_files: None,
    }))
}

fn refresh_frame_digest(bytes: &mut [u8]) {
    let digest = TestDigest.digest_parts(&[&bytes[4..50], &bytes[82..]]);
    bytes[50..82].copy_from_slice(
        digest
            .as_str()
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>()
            .as_slice(),
    );
}

#[test]
fn frame_round_trip_preserves_identity_payload_and_trusted_drain_facts() {
    let binding = digest('a');
    let message = sample((0..16384).map(|value| (value % 251) as u8).collect());
    let bytes = encode(&message, 17, &binding, &TestDigest).unwrap();
    assert!(bytes.len() <= 24576);
    let frame = decode(&bytes, &TestDigest).unwrap();
    assert_eq!(frame.identity.sequence, 17);
    assert_eq!(frame.identity.binding_digest, binding);
    assert_eq!(frame.message, message);
}

#[test]
fn frame_codec_rejects_zero_sequence_oversized_payload_and_tampering() {
    let binding = digest('a');
    assert_eq!(
        encode(&sample(Vec::new()), 0, &binding, &TestDigest),
        Err(CodecError::Value)
    );
    assert_eq!(
        encode(&sample(vec![0; 16385]), 1, &binding, &TestDigest),
        Err(CodecError::Length)
    );
    let invalid_eof = Message::Command(StorageCommand::Append {
        stream: Stream::Stdout,
        bytes: vec![1],
        drain: Some(TrustedDrainEvidence {
            reader_joined: true,
            accepted_data_reconciled: true,
            input_reconciled: true,
            stdout_eof: true,
            stderr_eof: true,
        }),
    });
    assert_eq!(
        encode(&invalid_eof, 1, &binding, &TestDigest),
        Err(CodecError::Length)
    );

    let mut bytes = encode(&sample(vec![1, 2, 3]), 1, &binding, &TestDigest).unwrap();
    bytes[90] ^= 0x40;
    assert_eq!(decode(&bytes, &TestDigest), Err(CodecError::Digest));
}

#[test]
fn zero_byte_eof_frame_preserves_all_three_reconciliation_facts() {
    let message = sample(Vec::new());
    let bytes = encode(&message, 1, &digest('a'), &TestDigest).unwrap();
    assert_eq!(decode(&bytes, &TestDigest).unwrap().message, message);
}

#[test]
fn frame_decoder_rejects_partial_and_oversized_frames_before_message_use() {
    let bytes = encode(&sample(vec![1, 2, 3]), 1, &digest('a'), &TestDigest).unwrap();
    assert_eq!(
        decode(&bytes[..bytes.len() - 1], &TestDigest),
        Err(CodecError::Length)
    );
    let mut oversized = bytes;
    oversized[0..4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(decode(&oversized, &TestDigest), Err(CodecError::Length));
}

#[test]
fn refusal_causes_round_trip_and_v1_unknown_or_contradictory_causes_are_rejected() {
    for cause in [StorageRefusal::Fenced, StorageRefusal::CommitLost] {
        let frame = encode(&refusal_message(cause), 1, &digest('a'), &TestDigest).unwrap();
        assert_eq!(frame[154], 1, "disposition byte");
        assert_eq!(
            frame[155],
            if cause == StorageRefusal::Fenced {
                1
            } else {
                2
            }
        );
        assert_eq!(
            decode(&frame, &TestDigest).unwrap().message,
            refusal_message(cause)
        );
    }

    let mut old = encode(
        &refusal_message(StorageRefusal::Fenced),
        1,
        &digest('a'),
        &TestDigest,
    )
    .unwrap();
    old[4..9].copy_from_slice(b"HFOR\x01");
    refresh_frame_digest(&mut old);
    assert_eq!(decode(&old, &TestDigest), Err(CodecError::Shape));

    let mut unknown = encode(
        &refusal_message(StorageRefusal::Fenced),
        1,
        &digest('a'),
        &TestDigest,
    )
    .unwrap();
    unknown[155] = 3;
    refresh_frame_digest(&mut unknown);
    assert_eq!(decode(&unknown, &TestDigest), Err(CodecError::Value));

    let Message::Ack(malformed) = refusal_message(StorageRefusal::Fenced) else {
        panic!("refusal helper must produce an acknowledgement");
    };
    let mut malformed = *malformed;
    malformed.refusal = None;
    assert_eq!(
        encode(
            &Message::Ack(Box::new(malformed)),
            1,
            &digest('a'),
            &TestDigest
        ),
        Err(CodecError::Value)
    );
    let mut malformed = match refusal_message(StorageRefusal::Fenced) {
        Message::Ack(ack) => *ack,
        _ => panic!("refusal helper must produce an acknowledgement"),
    };
    malformed.disposition = AckDisposition::Committed;
    assert_eq!(
        encode(
            &Message::Ack(Box::new(malformed)),
            1,
            &digest('a'),
            &TestDigest
        ),
        Err(CodecError::Value)
    );
}
