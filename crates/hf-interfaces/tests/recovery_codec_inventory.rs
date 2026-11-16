#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! String escaping and the inventory encoding of the recovery codec. Expected bytes were
//! produced by Python's `json.dumps` with `sort_keys=True, separators=(',', ':')` (the
//! reference engine's canonical form).
use hf_application::ctl::state_inspect::RecoveryCodecPort;
use hf_domain::ctl::recovery_proposal::{
    EntryBody, EntryStat, FileIdentity, Inventory, InventoryEntry,
};
use hf_interfaces::ctl::recovery_codec::{JsonRecoveryCodec, encode_inventory, write_string};
use std::collections::BTreeMap;

#[test]
fn strings_are_escaped_like_python_ensure_ascii() {
    let table: &[(&str, &str)] = &[
        ("", r#""""#),
        ("plain/path-1_2.3", r#""plain/path-1_2.3""#),
        ("a\"b", r#""a\"b""#),
        ("a\\b", r#""a\\b""#),
        ("a/b", r#""a/b""#),
        ("tab\u{9}here", r#""tab\there""#),
        ("nl\u{a}x", r#""nl\nx""#),
        ("cr\u{d}x", r#""cr\rx""#),
        ("bs\u{8}x", r#""bs\bx""#),
        ("ff\u{c}x", r#""ff\fx""#),
        ("nul\u{0}x", r#""nul\u0000x""#),
        ("us\u{1f}x", r#""us\u001fx""#),
        ("esc\u{1b}x", r#""esc\u001bx""#),
        ("del\u{7f}x", r#""del\u007fx""#),
        ("sp ace", r#""sp ace""#),
        ("\u{e9}", r#""\u00e9""#),
        ("\u{2028}\u{2029}", r#""\u2028\u2029""#),
        ("\u{ffff}", r#""\uffff""#),
        ("\u{10000}", r#""\ud800\udc00""#),
        ("\u{10ffff}", r#""\udbff\udfff""#),
        ("\u{1f600}x", r#""\ud83d\ude00x""#),
        ("\u{80}", r#""\u0080""#),
        ("\u{7ff}\u{800}", r#""\u07ff\u0800""#),
    ];
    for (text, expected) in table {
        let mut out = String::new();
        write_string(&mut out, text);
        assert_eq!(&out, expected, "{text:?}");
    }
}

fn entry(ino: u64, mode: u32, size: i64, mtime_ns: i64, body: EntryBody) -> InventoryEntry {
    InventoryEntry {
        stat: EntryStat {
            dev: 1,
            ino,
            mode,
            size,
            mtime_ns,
        },
        body,
    }
}

#[test]
fn inventory_names_sort_by_code_point_like_python() {
    let names = [
        "z",
        "a",
        "A",
        "\u{e9}",
        "\u{ffff}",
        "\u{10000}",
        "a b",
        "a\"b",
        "a\\b",
        "\u{7f}",
        "\u{1}",
        "~",
    ];
    let inventory = Inventory {
        root: FileIdentity { dev: 1, ino: 1 },
        lock: None,
        entries: names
            .iter()
            .map(|name| ((*name).to_string(), entry(2, 33_188, 4, 5, EntryBody::File)))
            .collect(),
    };
    // Every entry is [1,2,33188,4,5] in the Python document, so only names are compared.
    assert_eq!(
        encode_inventory(&inventory),
        concat!(
            r#"{"\u0001":[1,2,33188,4,5],"A":[1,2,33188,4,5],"a":[1,2,33188,4,5],"a b":[1,2"#,
            r#",33188,4,5],"a\"b":[1,2,33188,4,5],"a\\b":[1,2,33188,4,5],"z":[1,2,33188,4,5"#,
            r#"],"~":[1,2,33188,4,5],"\u007f":[1,2,33188,4,5],"\u00e9":[1,2,33188,4,5],"\uf"#,
            r#"fff":[1,2,33188,4,5],"\ud800\udc00":[1,2,33188,4,5]}"#,
        )
    );
}

#[test]
fn empty_inventories_and_directories_encode_as_empty_objects() {
    let empty = Inventory {
        root: FileIdentity { dev: 1, ino: 1 },
        lock: None,
        entries: BTreeMap::new(),
    };
    assert_eq!(encode_inventory(&empty), "{}");
    let with_directory = Inventory {
        entries: BTreeMap::from([(
            "d".to_string(),
            entry(2, 16_877, 4, 5, EntryBody::Directory(BTreeMap::new())),
        )]),
        ..empty
    };
    assert_eq!(
        encode_inventory(&with_directory),
        r#"{"d":[1,2,16877,4,5,{}]}"#
    );
}

#[test]
fn inventory_numbers_at_the_extremes_match_python() {
    let inventory = Inventory {
        root: FileIdentity { dev: 1, ino: 1 },
        lock: None,
        entries: BTreeMap::from([(
            "f".to_string(),
            InventoryEntry {
                stat: EntryStat {
                    dev: -1,
                    ino: i64::MAX as u64,
                    mode: 0,
                    size: i64::MAX,
                    mtime_ns: i64::MIN,
                },
                body: EntryBody::File,
            },
        )]),
    };
    assert_eq!(
        encode_inventory(&inventory),
        r#"{"f":[-1,9223372036854775807,0,9223372036854775807,-9223372036854775808]}"#
    );
}

#[test]
fn the_codec_port_returns_the_canonical_inventory_bytes() {
    let inventory = Inventory {
        root: FileIdentity { dev: 1, ino: 1 },
        lock: None,
        entries: BTreeMap::from([(
            "l".to_string(),
            entry(2, 41_453, 1, 3, EntryBody::Link("t\u{e9}".to_string())),
        )]),
    };
    assert_eq!(
        JsonRecoveryCodec.inventory_bytes(&inventory),
        encode_inventory(&inventory).into_bytes()
    );
    assert!(
        !JsonRecoveryCodec
            .inventory_bytes(&inventory)
            .ends_with(b"\n")
    );
}
