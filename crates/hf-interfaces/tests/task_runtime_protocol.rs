#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_application::task_runtime::InvalidIntent;
use hf_domain::{
    ownership::{Digest, Id, OwnerClaim},
    process::{ProcessExit, ProcessIdentity},
    task_lifecycle::LifecycleBinding,
    task_read::TaskId,
    task_runtime::{
        NotStarted, ProbeAttestation, SettlementAttestation, Started, WriterAcknowledgement,
        WriterCommand, WriterEnvelope, WriterOperation,
    },
};
use hf_interfaces::task_runtime::{
    decode_not_started, decode_started, decode_writer_ack, decode_writer_command,
    encode_not_started, encode_started, encode_writer_ack, encode_writer_command,
};

fn id(byte: u8) -> Id {
    Id::new(format!("{byte:032x}")).unwrap()
}

fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(
        pid,
        1_768_000_010,
        u64::from(pid),
        "01234567-89ab-cdef-0123-456789abcdef".into(),
    )
    .unwrap()
}

fn envelope() -> WriterEnvelope {
    WriterEnvelope {
        binding: LifecycleBinding {
            task: TaskId::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into()).unwrap(),
            operation: id(1),
            attempt: id(2),
            generation: id(3),
        },
        owner: OwnerClaim {
            owner: id(4),
            nonce: id(5),
            process: process(7101),
        },
        writer: process(7102),
        revision: 7,
        ownership_revision: 6,
    }
}

#[test]
fn capture_command_roundtrip_preserves_binding_and_payload_offset() {
    let command = WriterCommand {
        envelope: envelope(),
        operation: WriterOperation::Capture {
            stderr: true,
            offset: 19,
            bytes: b"captured".to_vec(),
        },
    };
    let bytes = encode_writer_command(&command).unwrap();
    assert_eq!(decode_writer_command(&bytes).unwrap(), command);
    let mut trailing = bytes;
    trailing.push(0);
    assert_eq!(decode_writer_command(&trailing), Err(InvalidIntent));
}

#[test]
fn finish_requires_observed_exit_and_digest_source_binding() {
    let proof = SettlementAttestation {
        child: process(7200),
        exit: ProcessExit::Code(17),
        stdout_end: 31,
        stderr_end: 9,
        input_bytes: 44,
        source_binding: Digest::new("a".repeat(64)).unwrap(),
        capture_loss: false,
    };
    let command = WriterCommand {
        envelope: envelope(),
        operation: WriterOperation::Finish {
            proof: proof.clone(),
            finished_at: "2026-01-10T10:05:00+00:00".into(),
        },
    };
    assert_eq!(
        decode_writer_command(&encode_writer_command(&command).unwrap()).unwrap(),
        command
    );
    let unobserved = WriterCommand {
        envelope: envelope(),
        operation: WriterOperation::Finish {
            proof: SettlementAttestation {
                exit: ProcessExit::Unobserved,
                ..proof
            },
            finished_at: "2026-01-10T10:05:00+00:00".into(),
        },
    };
    assert_eq!(encode_writer_command(&unobserved), Err(InvalidIntent));
}

#[test]
fn positive_started_and_acknowledgement_roundtrip_the_source_digest() {
    let binding = envelope();
    let source_binding = Digest::new("b".repeat(64)).unwrap();
    let started = Started {
        envelope: binding.clone(),
        child: process(7300),
        source_binding: source_binding.clone(),
    };
    let started_bytes = encode_started(&started).unwrap();
    assert_eq!(decode_started(&started_bytes).unwrap(), started);
    let ack = WriterAcknowledgement {
        envelope: binding,
        stdout_end: 21,
        stderr_end: 5,
        close_input: true,
        terminal: false,
        source_binding: Some(source_binding),
    };
    let bytes = encode_writer_ack(&ack).unwrap();
    assert_eq!(decode_writer_ack(&bytes).unwrap(), ack);
    let mut extra = bytes;
    extra.push(0);
    assert_eq!(decode_writer_ack(&extra), Err(InvalidIntent));
}

#[test]
fn empty_or_oversized_capture_and_zero_revision_are_refused() {
    for bytes in [Vec::new(), vec![b'x'; 16_385]] {
        let command = WriterCommand {
            envelope: envelope(),
            operation: WriterOperation::Capture {
                stderr: false,
                offset: 0,
                bytes,
            },
        };
        assert_eq!(encode_writer_command(&command), Err(InvalidIntent));
    }
    let mut invalid = envelope();
    invalid.revision = 0;
    let command = WriterCommand {
        envelope: invalid,
        operation: WriterOperation::Spawn {
            child: process(7400),
            started_at: "2026-01-10T10:00:00+00:00".into(),
        },
    };
    assert_eq!(encode_writer_command(&command), Err(InvalidIntent));
}

#[test]
fn unsupported_version_not_started_proof_is_typed_bounded_and_roundtrips() {
    let proof = ProbeAttestation {
        child: process(7500),
        exit: ProcessExit::Code(0),
        backend_digest: Digest::new("c".repeat(64)).unwrap(),
        stdout_bytes: 20,
        stderr_bytes: 0,
    };
    let receipt = NotStarted {
        envelope: envelope(),
        reason: "unsupported_version".into(),
        proof: proof.clone(),
    };
    let bytes = encode_not_started(&receipt).unwrap();
    assert_eq!(decode_not_started(&bytes).unwrap(), receipt);
    let command = WriterCommand {
        envelope: envelope(),
        operation: WriterOperation::NotStarted {
            proof: proof.clone(),
            reason: "unsupported_version".into(),
        },
    };
    assert_eq!(
        decode_writer_command(&encode_writer_command(&command).unwrap()).unwrap(),
        command
    );

    // Empty output can still be a settled probe with an unsupported version response.
    // The decoded envelope is evidence data only; it does not grant no-agent authority.
    let empty_output = NotStarted {
        envelope: envelope(),
        reason: "unsupported_version".into(),
        proof: ProbeAttestation {
            stdout_bytes: 0,
            ..proof.clone()
        },
    };
    let empty_bytes = encode_not_started(&empty_output).unwrap();
    assert_eq!(decode_not_started(&empty_bytes).unwrap(), empty_output);

    for invalid in [
        ProbeAttestation {
            exit: ProcessExit::Code(1),
            ..proof.clone()
        },
        ProbeAttestation {
            stdout_bytes: 4097,
            ..proof.clone()
        },
        ProbeAttestation {
            stderr_bytes: 1,
            ..proof
        },
    ] {
        assert_eq!(
            encode_not_started(&NotStarted {
                envelope: envelope(),
                reason: "unsupported_version".into(),
                proof: invalid,
            }),
            Err(InvalidIntent)
        );
    }
}
