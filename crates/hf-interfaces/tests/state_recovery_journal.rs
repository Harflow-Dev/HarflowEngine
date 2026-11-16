#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_application::ctl::state_recover::{
    evidence::{Basis, OwnerEvidence},
    journal::{INVENTORY_DEPTH_BOUND, JOURNAL_BOUND, JournalError, JournalPhase, RecoveryJournal},
};
use hf_domain::ctl::{
    identity::PRODUCT_VERSION,
    recovery_proposal::{
        EntryBody, EntryStat, FileIdentity, INVENTORY_BOUND, Inventory, InventoryEntry,
        RecoveryProposal,
    },
};
use hf_domain::process::{IdentityError, ProcessIdentity};
use hf_interfaces::ctl::recovery_journal::{decode_journal, encode_journal};
use std::collections::BTreeMap;

const OP: &str = "0123456789abcdef0123456789abcdef";
const ROOT: &str = "/tmp/état";
const BOOT: &str = "abcdef01-2345-6789-abcd-ef0123456789";
const ORACLE: &[u8] = include_bytes!("fixtures/state-recovery-planned-python.json");

fn id(ino: u64) -> FileIdentity {
    FileIdentity { dev: 7, ino }
}
fn entry(mode: u32, ino: u64, size: i64, body: EntryBody) -> InventoryEntry {
    InventoryEntry {
        stat: EntryStat {
            dev: 7,
            ino,
            mode,
            size,
            mtime_ns: 5,
        },
        body,
    }
}
fn oracle_journal() -> RecoveryJournal {
    let root = id(10);
    let lock = id(11);
    let mut entries = BTreeMap::new();
    entries.insert("café".into(), entry(0o100644, 30, 4, EntryBody::File));
    let proposal = RecoveryProposal {
        state_root: ROOT.into(),
        engine_version: PRODUCT_VERSION.into(),
        root_identity: root,
        lock_identity: Some(lock),
        inventory_sha256: "a".repeat(64),
        operation_id: OP.into(),
        prior_journal_sha256: Some(
            "06eb7d6a69ee19e5fbdf749018d3d2abfa04bcbd1365db312eb86dc7169389b8".into(),
        ),
    };
    RecoveryJournal {
        engine_version: PRODUCT_VERSION.into(),
        state_root: ROOT.into(),
        operation_id: OP.into(),
        backup_path: proposal.backup_path(),
        proposal,
        root_identity: root,
        lock_identity: lock,
        inventory: Inventory {
            root,
            lock: Some(lock),
            entries,
        },
        created_at: "2026-11-16T00:00:00Z".into(),
        phase: JournalPhase::Planned,
        backup_identity: None,
        data_identity: None,
        previous_journal_bytes: Some(vec![0, 255]),
        owner_evidence: OwnerEvidence {
            basis: Basis::EarlierSnapshot,
            checked: vec![ProcessIdentity::new(42, 123, 123_456, BOOT.into()).unwrap()],
        },
    }
}
fn replace_once(input: &[u8], from: &str, to: &str) -> Vec<u8> {
    let text = std::str::from_utf8(input).unwrap();
    assert!(text.contains(from), "missing mutation target: {from}");
    text.replacen(from, to, 1).into_bytes()
}
fn replace_inventory(input: &[u8], inventory: &str) -> Vec<u8> {
    let text = std::str::from_utf8(input).unwrap();
    let start = text.find("\"inventory\":").unwrap() + "\"inventory\":".len();
    let end = text[start..].find(",\"lock_identity\":").unwrap() + start;
    format!("{}{inventory}{}", &text[..start], &text[end..]).into_bytes()
}
fn nested_inventory_json(depth: usize) -> String {
    let mut value = "{}".to_string();
    for index in 0..depth {
        value = format!("{{\"d\":[7,{index},16877,0,5,{value}]}}");
    }
    value
}
fn empty_dir(depth: usize) -> BTreeMap<String, InventoryEntry> {
    let mut children = BTreeMap::new();
    for index in 0..depth {
        let mut parent = BTreeMap::new();
        parent.insert(
            "d".into(),
            entry(
                0o040755,
                index as u64 + 1,
                0,
                EntryBody::Directory(children),
            ),
        );
        children = parent;
    }
    children
}
fn flat_inventory(count: usize) -> BTreeMap<String, InventoryEntry> {
    (0..count)
        .map(|index| {
            (
                format!("f{index}"),
                entry(0o100644, index as u64 + 1, 0, EntryBody::File),
            )
        })
        .collect()
}

#[test]
fn python_pure_dictionary_oracle_matches_ensure_ascii_canonical_bytes() {
    assert_eq!(encode_journal(&oracle_journal()).unwrap(), ORACLE);
    assert_eq!(decode_journal(ORACLE).unwrap(), oracle_journal());
}

#[test]
fn decoder_accepts_whitespace_and_noncanonical_field_order_without_mutating_input() {
    let canonical = encode_journal(&oracle_journal()).unwrap();
    let body = std::str::from_utf8(&canonical)
        .unwrap()
        .strip_prefix('{')
        .unwrap()
        .strip_suffix(",\"v\":1}\n")
        .unwrap();
    let reordered = format!("{{ \"v\" : 1 , {body} }}").into_bytes();
    let original = reordered.clone();
    assert_eq!(decode_journal(&reordered).unwrap(), oracle_journal());
    assert_eq!(reordered, original);
    assert_eq!(
        encode_journal(&decode_journal(&reordered).unwrap()).unwrap(),
        canonical
    );
}

#[test]
fn decoder_rejects_duplicate_unknown_and_future_structural_fields() {
    let canonical = encode_journal(&oracle_journal()).unwrap();
    let duplicate = replace_once(&canonical, "\"v\":1}\n", "\"v\":1,\"v\":1}\n");
    assert_eq!(decode_journal(&duplicate), Err(JournalError::Invalid));
    let unknown = replace_once(&canonical, "\"v\":1}\n", "\"v\":1,\"staging\":null}\n");
    assert_eq!(
        decode_journal(&unknown),
        Err(JournalError::UnsupportedExtension)
    );
    let duplicate_proposal = replace_once(
        &canonical,
        "\"proposal\":{\"engine_version\"",
        "\"proposal\":{\"inventory_sha256\":\"x\",\"engine_version\"",
    );
    assert_eq!(
        decode_journal(&duplicate_proposal),
        Err(JournalError::Invalid)
    );
    let duplicate_evidence = replace_once(&canonical, "\"pid\":42", "\"pid\":42,\"pid\":42");
    assert_eq!(
        decode_journal(&duplicate_evidence),
        Err(JournalError::Invalid)
    );
    let duplicate_inventory = replace_inventory(
        &canonical,
        "{\"café\":[7,30,33188,4,5],\"café\":[7,30,33188,4,5]}",
    );
    assert_eq!(
        decode_journal(&duplicate_inventory),
        Err(JournalError::Invalid)
    );
    for phase in ["staging", "staged", "result", "committed", "completed"] {
        let input = replace_once(
            &canonical,
            "\"phase\":\"planned\"",
            &format!("\"phase\":\"{phase}\""),
        );
        assert_eq!(decode_journal(&input), Err(JournalError::UnsupportedPhase));
    }
    for extension in ["staging", "staged"] {
        let input = replace_once(
            &canonical,
            "\"v\":1}\n",
            &format!("\"v\":1,\"{extension}\":null}}\n"),
        );
        assert_eq!(
            decode_journal(&input),
            Err(JournalError::UnsupportedExtension)
        );
    }
}

#[test]
fn direct_decode_enforces_nested_depth_names_modes_arity_and_exclusions() {
    let canonical = encode_journal(&oracle_journal()).unwrap();
    let at_limit = replace_inventory(&canonical, &nested_inventory_json(INVENTORY_DEPTH_BOUND));
    assert!(decode_journal(&at_limit).is_ok());
    let beyond = replace_inventory(
        &canonical,
        &nested_inventory_json(INVENTORY_DEPTH_BOUND + 1),
    );
    assert_eq!(decode_journal(&beyond), Err(JournalError::TooLarge));
    for inventory in [
        "{\"store.lock\":[7,1,33188,0,5]}",
        "{\"outer\":[7,1,16877,0,5,{\"../escape\":[7,2,33188,0,5]}]}",
        "{\"x\":[7,1,33188,0,5,{}]}",
        "{\"x\":[7,1,33188]}",
    ] {
        assert_eq!(
            decode_journal(&replace_inventory(&canonical, inventory)),
            Err(JournalError::Invalid),
            "{inventory}"
        );
    }
    let nested_infrastructure = replace_inventory(
        &canonical,
        "{\"outer\":[7,1,16877,0,5,{\"store.lock\":[7,2,33188,0,5]}]}",
    );
    assert!(decode_journal(&nested_infrastructure).is_ok());
}

#[test]
fn malformed_json_numbers_base64_and_process_identities_refuse() {
    let canonical = encode_journal(&oracle_journal()).unwrap();
    for (from, to) in [
        ("\"v\":1", "\"v\":1.5"),
        ("[7,10]", "[7,-1]"),
        ("\"inventory_sha256\":\"", "\"inventory_sha256\":1e9999"),
        (
            "\"previous_journal_bytes\":\"AP8=\"",
            "\"previous_journal_bytes\":\"%%%\"",
        ),
    ] {
        assert!(
            decode_journal(&replace_once(&canonical, from, to)).is_err(),
            "{to}"
        );
    }
    assert_eq!(
        decode_journal(&canonical[..canonical.len() - 2]),
        Err(JournalError::Invalid)
    );
    let invalid_identity_fields = [
        ("\"pid\":42", "\"pid\":1"),
        ("\"start_seconds\":123", "\"start_seconds\":0"),
        (
            "\"start_microseconds\":123456",
            "\"start_microseconds\":1000000",
        ),
        (BOOT, "not-a-boot-uuid"),
    ];
    for (from, to) in invalid_identity_fields {
        assert_eq!(
            decode_journal(&replace_once(&canonical, from, to)),
            Err(JournalError::Invalid)
        );
    }
    assert_eq!(
        ProcessIdentity::new(1, 1, 0, BOOT.into()),
        Err(IdentityError::InvalidPid)
    );
    assert_eq!(
        ProcessIdentity::new(2, 0, 0, BOOT.into()),
        Err(IdentityError::InvalidStart)
    );
    assert_eq!(
        ProcessIdentity::new(2, 1, 0, "bad".into()),
        Err(IdentityError::InvalidBootUuid)
    );
}

#[test]
fn complete_codec_roundtrip_preserves_escaped_links_inventory_and_evidence() {
    let mut value = oracle_journal();
    value.inventory.entries.insert(
        "link".into(),
        entry(0o120777, 31, 8, EntryBody::Link("line\n\"é".into())),
    );
    value.previous_journal_bytes = Some((0..256).map(|n| n as u8).collect());
    let encoded = encode_journal(&value).unwrap();
    assert!(encoded.ends_with(b"}\n"));
    assert_eq!(decode_journal(&encoded).unwrap(), value);
}

#[test]
fn only_clear_and_earlier_snapshot_evidence_and_supported_precommit_phases_roundtrip() {
    for basis in [Basis::Clear, Basis::EarlierSnapshot] {
        for phase in [
            JournalPhase::Planned,
            JournalPhase::Prepared,
            JournalPhase::BackedUp,
        ] {
            let mut value = oracle_journal();
            value.owner_evidence.basis = basis;
            value.phase = phase;
            if phase != JournalPhase::Planned {
                value.backup_identity = Some(id(20));
                value.data_identity = Some(id(21));
            }
            let encoded = encode_journal(&value).unwrap();
            assert_eq!(decode_journal(&encoded).unwrap(), value);
        }
    }
    let unreadable = replace_once(
        &encode_journal(&oracle_journal()).unwrap(),
        "earlier_snapshot",
        "unreadable",
    );
    assert_eq!(decode_journal(&unreadable), Err(JournalError::Invalid));
}

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
