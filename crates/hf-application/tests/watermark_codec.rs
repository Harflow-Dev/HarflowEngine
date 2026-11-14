#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The `store.watermark` codec: payload bytes against an independent zlib vector, block
//! validity (magic, CRC, zero padding, zero filler), best-of-two selection with slot A
//! winning ties, and the whole-file length and generation rules.
use hf_application::watermark::{
    self, BLOCK_LEN, Decoded, FILE_LEN, NAME, PAYLOAD_LEN, Slot, any_valid, decode, decode_blocks,
    encode_payload, initial_file,
};

const GENERATION: &str = "0123456789abcdef0123456789abcdef";
const OTHER: &str = "fedcba9876543210fedcba9876543210";

/// Payload of `GENERATION` at seq 0x0102030405060708, built with Python's
/// `struct` and `zlib.crc32`.
const VECTOR: &str = "4846574d41524b3130313233343536373839616263646566303132333435363738396162\
636465660807060504030201000000000000000000000000a230dfd5";

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
        .collect()
}

fn block(seq: Option<u64>, generation: &str) -> Vec<u8> {
    let mut block = vec![0u8; BLOCK_LEN];
    if let Some(seq) = seq {
        block[..PAYLOAD_LEN].copy_from_slice(&encode_payload(generation, seq).unwrap());
    }
    block
}

fn file(a: Option<u64>, b: Option<u64>) -> Vec<u8> {
    [block(a, GENERATION), block(b, GENERATION)].concat()
}

fn valid(best: Slot, seq: u64) -> Decoded {
    Decoded::Valid { best, seq }
}

#[test]
fn the_payload_matches_an_independent_vector_and_round_trips() {
    let payload = encode_payload(GENERATION, 0x0102_0304_0506_0708).unwrap();
    assert_eq!(payload.to_vec(), hex(VECTOR));
    assert_eq!(watermark::crc32(b""), 0);
    assert_eq!(watermark::crc32(b"123456789"), 0xCBF4_3926);
    for seq in [0, 1, 2, u64::from(u32::MAX) + 1, u64::MAX] {
        let bytes = file(Some(seq), None);
        assert_eq!(decode(&bytes, GENERATION), valid(Slot::A, seq), "{seq}");
        let bytes = file(None, Some(seq));
        assert_eq!(decode(&bytes, GENERATION), valid(Slot::B, seq), "{seq}");
    }
    assert_eq!(
        (FILE_LEN, BLOCK_LEN, PAYLOAD_LEN, NAME),
        (8192, 4096, 64, "store.watermark")
    );
}

#[test]
fn only_a_lowercase_hex_generation_of_32_characters_encodes() {
    for bad in [
        "",
        "ABCDEF0123456789abcdef0123456789",
        &GENERATION[..31],
        "g".repeat(32).as_str(),
    ] {
        assert!(encode_payload(bad, 0).is_none(), "{bad:?}");
        assert!(initial_file(bad).is_none(), "{bad:?}");
    }
    assert!(encode_payload(&format!("{GENERATION}0"), 0).is_none());
}

#[test]
fn any_damage_to_a_block_makes_only_that_block_invalid() {
    let good = file(Some(11), Some(10));
    assert_eq!(decode(&good, GENERATION), valid(Slot::A, 11));
    // Magic, generation, sequence, zero padding, CRC, and the filler after the payload.
    for at in [0, 7, 8, 39, 40, 47, 48, 59, 60, 63, 64, 1000, BLOCK_LEN - 1] {
        let mut damaged = good.clone();
        damaged[at] ^= 0x01;
        assert_eq!(
            decode(&damaged, GENERATION),
            valid(Slot::B, 10),
            "byte {at}"
        );
        let mut damaged = good.clone();
        damaged[BLOCK_LEN + at] ^= 0x80;
        assert_eq!(
            decode(&damaged, GENERATION),
            valid(Slot::A, 11),
            "byte {at} of B"
        );
    }
}

#[test]
fn a_block_with_a_correct_crc_but_nonzero_padding_is_invalid() {
    let mut payload = encode_payload(GENERATION, 5).unwrap();
    payload[50] = 1;
    let crc = watermark::crc32(&payload[..60]);
    payload[60..].copy_from_slice(&crc.to_le_bytes());
    let mut bytes = file(None, Some(4));
    bytes[..PAYLOAD_LEN].copy_from_slice(&payload);
    assert_eq!(decode(&bytes, GENERATION), valid(Slot::B, 4));
}

#[test]
fn equal_sequences_make_slot_a_best_so_slot_b_is_written_next() {
    let initial = initial_file(GENERATION).unwrap();
    assert_eq!(initial.len(), FILE_LEN);
    assert_eq!(initial, file(Some(0), Some(0)));
    assert_eq!(decode(&initial, GENERATION), valid(Slot::A, 0));
    assert_eq!(Slot::A.other(), Slot::B);
    assert_eq!(Slot::B.other(), Slot::A);
    assert_eq!((Slot::A.offset(), Slot::B.offset()), (0, 4096));
    assert_eq!(
        decode(&file(Some(7), Some(7)), GENERATION),
        valid(Slot::A, 7)
    );
}

#[test]
fn the_higher_valid_sequence_wins_in_either_slot() {
    assert_eq!(
        decode(&file(Some(9), Some(10)), GENERATION),
        valid(Slot::B, 10)
    );
    assert_eq!(
        decode(&file(Some(11), Some(10)), GENERATION),
        valid(Slot::A, 11)
    );
    assert_eq!(
        decode(&file(Some(0), Some(u64::MAX)), GENERATION),
        valid(Slot::B, u64::MAX)
    );
    assert_eq!(decode(&file(Some(3), None), GENERATION), valid(Slot::A, 3));
    assert_eq!(decode(&file(None, Some(3)), GENERATION), valid(Slot::B, 3));
}

#[test]
fn no_valid_block_is_unavailable_and_any_valid_says_so() {
    let empty = file(None, None);
    assert_eq!(decode(&empty, GENERATION), Decoded::Unavailable);
    assert!(!any_valid(&empty));
    let mut torn = file(Some(1), Some(2));
    torn[44] ^= 1;
    torn[BLOCK_LEN + 44] ^= 1;
    assert_eq!(decode(&torn, GENERATION), Decoded::Unavailable);
    assert!(!any_valid(&torn));
    assert!(any_valid(&file(None, Some(2))));
    // `any_valid` ignores the generation and requires the exact length.
    assert!(any_valid(
        &[block(Some(1), OTHER), block(None, OTHER)].concat()
    ));
    assert!(!any_valid(&file(Some(1), None)[..FILE_LEN - 1]));
}

#[test]
fn a_wrong_length_or_a_valid_foreign_slot_is_incompatible() {
    let good = file(Some(1), Some(2));
    for len in [0, 64, 128, BLOCK_LEN, FILE_LEN - 1] {
        assert_eq!(
            decode(&good[..len], GENERATION),
            Decoded::Incompatible,
            "{len}"
        );
    }
    let long = [good.as_slice(), &[0]].concat();
    assert_eq!(decode(&long, GENERATION), Decoded::Incompatible);
    let foreign_a = [block(Some(1), OTHER), block(Some(9), GENERATION)].concat();
    assert_eq!(decode(&foreign_a, GENERATION), Decoded::Incompatible);
    let foreign_b = [block(Some(9), GENERATION), block(Some(1), OTHER)].concat();
    assert_eq!(decode(&foreign_b, GENERATION), Decoded::Incompatible);
    // An invalid block naming another generation is a torn write, not a foreign file.
    let mut torn_foreign = block(Some(1), OTHER);
    torn_foreign[60] ^= 1;
    let mixed = [torn_foreign, block(Some(9), GENERATION)].concat();
    assert_eq!(decode(&mixed, GENERATION), valid(Slot::B, 9));
}

#[test]
fn decode_blocks_treats_a_block_of_the_wrong_length_as_invalid() {
    let a = block(Some(4), GENERATION);
    let b = block(Some(3), GENERATION);
    assert_eq!(decode_blocks(&a, &b, GENERATION), valid(Slot::A, 4));
    assert_eq!(
        decode_blocks(&a[..PAYLOAD_LEN], &b, GENERATION),
        valid(Slot::B, 3)
    );
    let long = [a.as_slice(), &[0]].concat();
    assert_eq!(decode_blocks(&long, &b, GENERATION), valid(Slot::B, 3));
    assert_eq!(decode_blocks(&[], &[], GENERATION), Decoded::Unavailable);
}
