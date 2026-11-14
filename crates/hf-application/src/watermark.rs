//! Codec of `store.watermark`, the commit watermark of a state root. Pure: no I/O.
//!
//! The file is exactly `FILE_LEN` (8192) bytes: two 4 KiB blocks, slot A at offset 0 and
//! slot B at offset 4096. Each block starts with a 64-byte payload, little-endian:
//! `[0..8)` magic `HFWMARK1`, `[8..40)` the generation as 32 lowercase hex characters,
//! `[40..48)` the commit sequence as u64, `[48..60)` zero, `[60..64)` CRC-32 (IEEE) of
//! `[0..60)`. The rest of the block is zero. A block is valid when magic, CRC, zero
//! padding and zero filler all hold; an invalid block is a torn or never-completed write
//! and is ignored. The best valid block holds the highest sequence; on equal sequences
//! slot A is the best. The storage writer and the lease-free inspector share this codec.

/// Exact length of the file.
pub const FILE_LEN: usize = 8192;
/// Length of one block; each slot owns one.
pub const BLOCK_LEN: usize = 4096;
/// Length of the payload at the start of each block.
pub const PAYLOAD_LEN: usize = 64;
/// File name in the state root.
pub const NAME: &str = "store.watermark";
const MAGIC: &[u8; 8] = b"HFWMARK1";
const GENERATION_LEN: usize = 32;

/// One of the two slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    A,
    B,
}
impl Slot {
    /// Byte offset of the slot's block.
    pub fn offset(self) -> u64 {
        match self {
            Self::A => 0,
            Self::B => BLOCK_LEN as u64,
        }
    }
    /// The other slot: the one a writer overwrites when this one is the best.
    pub fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

/// What both blocks say about one generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decoded {
    /// The best valid block and its sequence.
    Valid { best: Slot, seq: u64 },
    /// Neither block is valid.
    Unavailable,
    /// The file has the wrong length, or a valid block names another generation.
    Incompatible,
}

/// The payload for `generation` at `seq`; `None` unless `generation` is 32 lowercase hex
/// characters.
pub fn encode_payload(generation: &str, seq: u64) -> Option<[u8; PAYLOAD_LEN]> {
    if !is_generation(generation.as_bytes()) {
        return None;
    }
    let mut payload = [0u8; PAYLOAD_LEN];
    payload[..8].copy_from_slice(MAGIC);
    payload[8..40].copy_from_slice(generation.as_bytes());
    payload[40..48].copy_from_slice(&seq.to_le_bytes());
    let crc = crc32(&payload[..60]);
    payload[60..].copy_from_slice(&crc.to_le_bytes());
    Some(payload)
}

/// A whole new file: both slots at sequence 0.
pub fn initial_file(generation: &str) -> Option<Vec<u8>> {
    let payload = encode_payload(generation, 0)?;
    let mut file = vec![0u8; FILE_LEN];
    file[..PAYLOAD_LEN].copy_from_slice(&payload);
    file[BLOCK_LEN..BLOCK_LEN + PAYLOAD_LEN].copy_from_slice(&payload);
    Some(file)
}

/// The generation bytes and sequence of a valid block, `None` for an invalid one.
fn entry(block: &[u8]) -> Option<(&[u8], u64)> {
    if block.len() != BLOCK_LEN || block.get(..8)? != MAGIC {
        return None;
    }
    if block.get(48..60)?.iter().any(|byte| *byte != 0)
        || block.get(PAYLOAD_LEN..)?.iter().any(|byte| *byte != 0)
    {
        return None;
    }
    let stored = u32::from_le_bytes(block.get(60..64)?.try_into().ok()?);
    if stored != crc32(block.get(..60)?) {
        return None;
    }
    let seq = u64::from_le_bytes(block.get(40..48)?.try_into().ok()?);
    Some((block.get(8..40)?, seq))
}

/// Decodes the two blocks, each exactly `BLOCK_LEN` bytes (anything else is invalid).
pub fn decode_blocks(a: &[u8], b: &[u8], generation: &str) -> Decoded {
    let mut best: Option<(Slot, u64)> = None;
    for (slot, block) in [(Slot::A, a), (Slot::B, b)] {
        let Some((stored, seq)) = entry(block) else {
            continue;
        };
        if stored != generation.as_bytes() {
            return Decoded::Incompatible;
        }
        // Strictly greater: on equal sequences slot A stays the best.
        if best.is_none_or(|(_, current)| seq > current) {
            best = Some((slot, seq));
        }
    }
    match best {
        Some((best, seq)) => Decoded::Valid { best, seq },
        None => Decoded::Unavailable,
    }
}

/// Decodes a whole file; a length other than `FILE_LEN` is `Incompatible`.
pub fn decode(file: &[u8], generation: &str) -> Decoded {
    if file.len() != FILE_LEN {
        return Decoded::Incompatible;
    }
    let (a, b) = file.split_at(BLOCK_LEN);
    decode_blocks(a, b, generation)
}

/// Whether a whole file of `FILE_LEN` bytes has at least one valid block, whatever its
/// generation. Lets an observer without the generation retry a torn read.
pub fn any_valid(file: &[u8]) -> bool {
    file.len() == FILE_LEN && file.chunks(BLOCK_LEN).any(|block| entry(block).is_some())
}

fn is_generation(bytes: &[u8]) -> bool {
    bytes.len() == GENERATION_LEN
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

const CRC_TABLE: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 == 1 {
                (value >> 1) ^ 0xEDB8_8320
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

/// CRC-32 (IEEE 802.3, as zlib's `crc32`).
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc = CRC_TABLE[((crc ^ u32::from(*byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    const GENERATION: &str = "0123456789abcdef0123456789abcdef";
    const OTHER: &str = "fedcba9876543210fedcba9876543210";

    fn file(a: Option<u64>, b: Option<u64>) -> Vec<u8> {
        let mut bytes = vec![0u8; FILE_LEN];
        if let Some(seq) = a {
            bytes[..PAYLOAD_LEN].copy_from_slice(&encode_payload(GENERATION, seq).unwrap());
        }
        if let Some(seq) = b {
            bytes[BLOCK_LEN..BLOCK_LEN + PAYLOAD_LEN]
                .copy_from_slice(&encode_payload(GENERATION, seq).unwrap());
        }
        bytes
    }

    #[test]
    fn crc_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn the_initial_file_is_seq_zero_with_slot_a_best() {
        let bytes = initial_file(GENERATION).unwrap();
        let expected = Decoded::Valid {
            best: Slot::A,
            seq: 0,
        };
        assert_eq!(decode(&bytes, GENERATION), expected);
    }

    #[test]
    fn the_highest_valid_slot_wins_and_torn_or_foreign_blocks_are_handled() {
        let valid = |best, seq| Decoded::Valid { best, seq };
        assert_eq!(
            decode(&file(Some(9), Some(10)), GENERATION),
            valid(Slot::B, 10)
        );
        assert_eq!(
            decode(&file(Some(11), Some(10)), GENERATION),
            valid(Slot::A, 11)
        );
        let mut torn = file(Some(11), Some(10));
        torn[45] ^= 1;
        assert_eq!(decode(&torn, GENERATION), valid(Slot::B, 10));
        let mut filler = file(Some(11), Some(10));
        filler[100] = 1;
        assert_eq!(decode(&filler, GENERATION), valid(Slot::B, 10));
        assert_eq!(decode(&file(None, None), GENERATION), Decoded::Unavailable);
        assert!(!any_valid(&file(None, None)));
        assert_eq!(decode(&file(Some(1), None), OTHER), Decoded::Incompatible);
        assert_eq!(decode(&[0u8; 128], GENERATION), Decoded::Incompatible);
        assert!(encode_payload("ABC", 0).is_none());
    }
}
