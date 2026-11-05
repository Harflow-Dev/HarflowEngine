#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_application::owner_runtime::DigestPort;
use hf_domain::{
    owner_runtime::{StorageCommand, Stream, TrustedDrainEvidence},
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
