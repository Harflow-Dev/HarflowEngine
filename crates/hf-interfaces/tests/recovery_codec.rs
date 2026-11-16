#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! The proposal token and inventory encodings against the reference engine's bytes.
use hf_domain::ctl::recovery_proposal::{
    EntryBody, EntryStat, FileIdentity, Inventory, InventoryEntry, RecoveryProposal,
};
use hf_interfaces::ctl::recovery_codec::{
    StaleToken, decode_token, encode_inventory, encode_proposal, encode_token,
};
use std::collections::BTreeMap;

fn proposal() -> RecoveryProposal {
    RecoveryProposal {
        state_root: "/tmp/\u{e9}tat/\u{4e2d}\u{6587}/\u{1f600}\"\\\n\u{7f}".to_string(),
        engine_version: "0.0.13+rust".to_string(),
        root_identity: FileIdentity {
            dev: 16_777_234,
            ino: 12_345,
        },
        lock_identity: None,
        inventory_sha256: "a".repeat(64),
        operation_id: "0123456789abcdef0123456789abcdef".to_string(),
        prior_journal_sha256: Some("b".repeat(64)),
    }
}

// Produced by Python's `json.dumps(..., sort_keys=True, separators=(',', ':'))`.
const PYTHON_JSON: &str = concat!(
    r#"{"engine_version":"0.0.13+rust","inventory_sha256":""#,
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    r#"","lock_identity":null,"operation_id":"0123456789abcdef0123456789abcdef","#,
    r#""prior_journal_sha256":""#,
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    r#"","root_identity":[16777234,12345],"#,
    r#""state_root":"/tmp/\u00e9tat/\u4e2d\u6587/\ud83d\ude00\"\\\n\u007f"}"#,
);

#[test]
fn token_round_trips_canonical_with_ensure_ascii_escaping() {
    let proposal = proposal();
    assert_eq!(encode_proposal(&proposal), PYTHON_JSON);
    let token = encode_token(&proposal);
    assert_eq!(
        token,
        "eyJlbmdpbmVfdmVyc2lvbiI6IjAuMC4xMytydXN0IiwiaW52ZW50b3J5X3NoYTI1NiI6ImFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWEiLCJsb2NrX2lkZW50aXR5IjpudWxsLCJvcGVyYXRpb25faWQiOiIwMTIzNDU2Nzg5YWJjZGVmMDEyMzQ1Njc4OWFiY2RlZiIsInByaW9yX2pvdXJuYWxfc2hhMjU2IjoiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYiIsInJvb3RfaWRlbnRpdHkiOlsxNjc3NzIzNCwxMjM0NV0sInN0YXRlX3Jvb3QiOiIvdG1wL1x1MDBlOXRhdC9cdTRlMmRcdTY1ODcvXHVkODNkXHVkZTAwXCJcXFxuXHUwMDdmIn0"
    );
    assert_eq!(decode_token(&token), Ok(proposal));
}

#[test]
fn token_over_8192_or_noncanonical_is_stale() {
    let proposal = proposal();
    let token = encode_token(&proposal);
    assert_eq!(decode_token(&"A".repeat(8193)), Err(StaleToken));
    assert_eq!(decode_token(&format!("{token}=")), Err(StaleToken));
    assert_eq!(decode_token("not a token"), Err(StaleToken));
    // Valid JSON of the same content with a different spelling is not canonical.
    let spaced = encode_proposal(&proposal).replace(',', ", ");
    let encoded = base64_url(spaced.as_bytes());
    assert_eq!(decode_token(&encoded), Err(StaleToken));
    // An extra key is rejected too.
    let extra = encode_proposal(&proposal).replace("{", "{\"a\":1,");
    assert_eq!(decode_token(&base64_url(extra.as_bytes())), Err(StaleToken));
    // A token of exactly 8192 characters that is not a proposal is stale, not accepted.
    assert_eq!(decode_token(&"A".repeat(8192)), Err(StaleToken));
}

fn base64_url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let word = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[((word >> (18 - 6 * i)) & 63) as usize]));
        }
    }
    out
}

#[test]
fn inventory_encoding_matches_python() {
    let stat = |ino, mode, size, mtime_ns| EntryStat {
        dev: 1,
        ino,
        mode,
        size,
        mtime_ns,
    };
    let link = InventoryEntry {
        stat: stat(4, 41_453, 3, 8),
        body: EntryBody::Link("tg\u{1f600}".to_string()),
    };
    let directory = InventoryEntry {
        stat: stat(3, 16_877, 64, 7),
        body: EntryBody::Directory(BTreeMap::from([("l".to_string(), link)])),
    };
    let file = InventoryEntry {
        stat: stat(2, 33_188, 5, 6),
        body: EntryBody::File,
    };
    let inventory = Inventory {
        root: FileIdentity { dev: 1, ino: 1 },
        lock: None,
        entries: BTreeMap::from([("d \u{e9}".to_string(), directory), ("a".to_string(), file)]),
    };
    assert_eq!(
        encode_inventory(&inventory),
        r#"{"a":[1,2,33188,5,6],"d \u00e9":[1,3,16877,64,7,{"l":[1,4,41453,3,8,"tg\ud83d\ude00"]}]}"#
    );
}
