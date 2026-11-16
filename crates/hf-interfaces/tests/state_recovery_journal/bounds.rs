#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;

#[test]
fn decoder_and_encoder_enforce_inventory_depth_and_entry_count() {
    for depth in [INVENTORY_DEPTH_BOUND, INVENTORY_DEPTH_BOUND + 1] {
        let mut value = oracle_journal();
        value.inventory.entries = empty_dir(depth);
        let outcome = encode_journal(&value);
        if depth == INVENTORY_DEPTH_BOUND {
            assert!(decode_journal(&outcome.unwrap()).is_ok());
        } else {
            assert_eq!(outcome, Err(JournalError::TooLarge));
        }
    }
    let mut valid = oracle_journal();
    valid.inventory.entries = flat_inventory(INVENTORY_BOUND);
    let bytes = encode_journal(&valid).unwrap();
    assert_eq!(decode_journal(&bytes).unwrap(), valid);
    let text = std::str::from_utf8(&bytes).unwrap();
    let (prefix, suffix) = text.split_once(",\"lock_identity\":").unwrap();
    let inventory = prefix.strip_suffix('}').unwrap();
    let too_many =
        format!("{inventory},\"f100000\":[7,100001,33188,0,5]}},\"lock_identity\":{suffix}");
    assert_eq!(
        decode_journal(too_many.as_bytes()),
        Err(JournalError::TooLarge)
    );
    let mut invalid = oracle_journal();
    invalid.inventory.entries = flat_inventory(INVENTORY_BOUND + 1);
    assert_eq!(encode_journal(&invalid), Err(JournalError::TooLarge));
}

#[test]
fn full_encoded_bound_includes_escaping_base64_and_final_newline() {
    let mut base = oracle_journal();
    let canonical = encode_journal(&base).unwrap();
    assert!(canonical.len() < JOURNAL_BOUND);
    let mut exact = None;
    for pad in 0..4 {
        base.created_at = format!("2026-11-16T00:00:00Z{}", "x".repeat(pad));
        let mut low = 0usize;
        let mut high = JOURNAL_BOUND;
        while low <= high {
            let mid = low + (high - low) / 2;
            base.previous_journal_bytes = Some(vec![0xff; mid]);
            match encode_journal(&base) {
                Ok(bytes) => {
                    assert!(bytes.len() <= JOURNAL_BOUND);
                    if bytes.len() == JOURNAL_BOUND {
                        exact = Some((base.clone(), bytes));
                        break;
                    }
                    low = mid + 1;
                }
                _ => {
                    if mid == 0 {
                        break;
                    }
                    high = mid - 1;
                }
            }
        }
        if exact.is_some() {
            break;
        }
    }
    let (exact_value, bytes) = exact.expect("canonical output should reach the exact byte bound");
    assert_eq!(bytes.len(), JOURNAL_BOUND);
    assert!(bytes.ends_with(b"}\n"));
    assert_eq!(decode_journal(&bytes).unwrap(), exact_value);
    assert_eq!(
        decode_journal(&[bytes.as_slice(), b" "].concat()),
        Err(JournalError::TooLarge)
    );
    let mut over = exact_value;
    over.previous_journal_bytes
        .as_mut()
        .unwrap()
        .extend_from_slice(&[1, 2, 3]);
    assert_eq!(encode_journal(&over), Err(JournalError::TooLarge));
}
