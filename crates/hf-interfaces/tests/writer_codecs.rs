#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_application::task_runtime::InvalidIntent;
use hf_domain::{
    ownership::{Digest, Id, OwnerClaim},
    process::{ProcessExit, ProcessIdentity},
    task_lifecycle::LifecycleBinding,
    task_read::TaskId,
    task_runtime::{
        CaptureLoss, SettlementAttestation, WriterCommand, WriterEnvelope, WriterOperation,
    },
};
use hf_interfaces::task_runtime::{decode_writer_command, encode_writer_command};

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
fn command(loss: CaptureLoss, exit: ProcessExit) -> WriterCommand {
    WriterCommand {
        envelope: WriterEnvelope {
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
        },
        operation: WriterOperation::Finish {
            proof: SettlementAttestation {
                child: process(7200),
                exit,
                stdout_end: 31,
                stderr_end: 9,
                input_bytes: 44,
                source_binding: Digest::new("a".repeat(64)).unwrap(),
                capture_loss: loss,
            },
            finished_at: "2026-01-10T10:05:00+00:00".into(),
        },
    }
}
/// Index of the last byte of the opcode: the only byte the two encodings differ in.
fn opcode_index(plain: &[u8], lossy: &[u8]) -> usize {
    assert_eq!(plain.len(), lossy.len());
    let differing: Vec<_> = (0..plain.len())
        .filter(|i| plain[*i] != lossy[*i])
        .collect();
    assert_eq!(differing.len(), 1);
    differing[0]
}

#[test]
fn opcode_two_bytes_are_pinned_and_opcode_four_differs_only_in_the_opcode() {
    let plain = encode_writer_command(&command(CaptureLoss::None, ProcessExit::Code(0))).unwrap();
    let lossy = encode_writer_command(&command(CaptureLoss::Stall, ProcessExit::Code(0))).unwrap();
    let at = opcode_index(&plain, &lossy);
    assert_eq!(plain[at - 7..=at], [0, 0, 0, 0, 0, 0, 0, 2]);
    assert_eq!(lossy[at - 7..=at], [0, 0, 0, 0, 0, 0, 0, 4]);
    assert_eq!(&plain[..5], b"HFTQ\x01");
    assert_eq!(
        decode_writer_command(&plain).unwrap(),
        command(CaptureLoss::None, ProcessExit::Code(0))
    );
    assert_eq!(
        decode_writer_command(&lossy).unwrap(),
        command(CaptureLoss::Stall, ProcessExit::Code(0))
    );
}

#[test]
fn truncated_and_extended_loss_frames_are_refused() {
    let lossy = encode_writer_command(&command(CaptureLoss::Stall, ProcessExit::Code(1))).unwrap();
    for cut in [1, 8, 40] {
        assert_eq!(
            decode_writer_command(&lossy[..lossy.len() - cut]),
            Err(InvalidIntent)
        );
    }
    let mut extended = lossy;
    extended.push(0);
    assert_eq!(decode_writer_command(&extended), Err(InvalidIntent));
}

#[test]
fn invalid_exit_in_a_loss_frame_is_refused() {
    let plain = encode_writer_command(&command(CaptureLoss::None, ProcessExit::Code(0))).unwrap();
    let lossy = encode_writer_command(&command(CaptureLoss::Stall, ProcessExit::Code(0))).unwrap();
    let at = opcode_index(&plain, &lossy);
    // After the opcode: pid, seconds, microseconds (8 bytes each), the boot text (4 + 36),
    // then the exit kind and code (8 bytes each).
    let kind_end = at + 1 + 24 + 4 + 36 + 8;
    for (kind, code) in [(1u8, 0u8), (2, 0)] {
        let mut frame = lossy.clone();
        frame[kind_end - 1] = kind;
        frame[kind_end + 7] = code;
        assert_eq!(
            decode_writer_command(&frame),
            Err(InvalidIntent),
            "kind {kind}"
        );
    }
    let mut unobserved = command(CaptureLoss::Stall, ProcessExit::Code(0));
    if let WriterOperation::Finish { proof, .. } = &mut unobserved.operation {
        proof.exit = ProcessExit::Unobserved;
    }
    assert_eq!(encode_writer_command(&unobserved), Err(InvalidIntent));
}

/// The complete opcode-2 command as the encoder produced it before opcode 4 existed.
const GOLDEN_OPCODE_TWO: &str = concat!(
    "48465451010000002461616161616161612d616161612d346161612d386161612d616161616161616161616161000000",
    "203030303030303030303030303030303030303030303030303030303030303031000000203030303030303030303030",
    "303030303030303030303030303030303030303032000000203030303030303030303030303030303030303030303030",
    "303030303030303033000000203030303030303030303030303030303030303030303030303030303030303034000000",
    "2030303030303030303030303030303030303030303030303030303030303030350000000000001bbd0000000069618a",
    "0a0000000000001bbd0000002430313233343536372d383961622d636465662d303132332d3435363738396162636465",
    "660000000000001bbe0000000069618a0a0000000000001bbe0000002430313233343536372d383961622d636465662d",
    "303132332d3435363738396162636465660000000000000007000000000000000600000000000000020000000000001c",
    "200000000069618a0a0000000000001c200000002430313233343536372d383961622d636465662d303132332d343536",
    "37383961626364656600000000000000000000000000000000000000000000001f000000000000000900000000000000",
    "2c0000004061616161616161616161616161616161616161616161616161616161616161616161616161616161616161",
    "61616161616161616161616161616161616161616100000019323032362d30312d31305431303a30353a30302b30303a",
    "3030",
);

#[test]
fn opcode_two_golden_vector_is_unchanged() {
    let bytes = encode_writer_command(&command(CaptureLoss::None, ProcessExit::Code(0))).unwrap();
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, GOLDEN_OPCODE_TWO);
}

#[test]
fn finish_opcode5_golden_roundtrip() {
    let plain = encode_writer_command(&command(CaptureLoss::None, ProcessExit::Code(0))).unwrap();
    let stall = encode_writer_command(&command(CaptureLoss::Stall, ProcessExit::Code(0))).unwrap();
    let disconnect =
        encode_writer_command(&command(CaptureLoss::Disconnect, ProcessExit::Code(0))).unwrap();
    let at = opcode_index(&plain, &disconnect);
    assert_eq!(disconnect[at - 7..=at], [0, 0, 0, 0, 0, 0, 0, 5]);
    assert_eq!(stall[at], 4);
    assert_eq!(
        decode_writer_command(&disconnect).unwrap(),
        command(CaptureLoss::Disconnect, ProcessExit::Code(0))
    );
    assert_eq!(
        decode_writer_command(&stall).unwrap(),
        command(CaptureLoss::Stall, ProcessExit::Code(0))
    );
}

#[test]
fn finish_opcode6_refused() {
    let plain = encode_writer_command(&command(CaptureLoss::None, ProcessExit::Code(0))).unwrap();
    let lossy = encode_writer_command(&command(CaptureLoss::Stall, ProcessExit::Code(0))).unwrap();
    let at = opcode_index(&plain, &lossy);
    for opcode in [3u8, 6, 7] {
        let mut frame = lossy.clone();
        frame[at] = opcode;
        assert_eq!(
            decode_writer_command(&frame),
            Err(InvalidIntent),
            "opcode {opcode}"
        );
    }
}
