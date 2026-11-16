#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Fresh operation identifiers.
use hf_application::ctl::state_inspect::OperationIdPort;
use hf_domain::ctl::state_recovery::is_operation_id;
use hf_platform::ctl::operation_id::OsOperationIds;
use std::collections::HashSet;

#[test]
fn identifiers_are_32_lowercase_hexadecimal_digits() {
    for _ in 0..100 {
        let id = OsOperationIds.fresh().unwrap();
        assert_eq!(id.len(), 32);
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "{id}"
        );
        assert!(is_operation_id(&id));
    }
}

#[test]
fn identifiers_do_not_repeat() {
    let ids: HashSet<String> = (0..1000).map(|_| OsOperationIds.fresh().unwrap()).collect();
    assert_eq!(ids.len(), 1000);
}

#[test]
fn identifiers_use_the_whole_alphabet_and_both_nibbles() {
    // 16 random bytes per id: over many ids every digit appears in both positions.
    let mut first = HashSet::new();
    let mut second = HashSet::new();
    for _ in 0..2000 {
        let id = OsOperationIds.fresh().unwrap();
        for pair in id.as_bytes().chunks(2) {
            first.insert(pair[0]);
            second.insert(pair[1]);
        }
    }
    assert_eq!(first.len(), 16);
    assert_eq!(second.len(), 16);
}
