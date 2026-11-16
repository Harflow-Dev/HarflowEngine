#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Edges of the recovery codec: token bounds, rejections and round trips. Expected bytes were
//! produced by Python's `json.dumps` with
//! `sort_keys=True, separators=(',', ':')` (the reference engine's canonical form).
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hf_domain::ctl::recovery_proposal::{FileIdentity, RecoveryProposal, TOKEN_BOUND};
use hf_interfaces::ctl::recovery_codec::{
    StaleToken, decode_token, encode_proposal, encode_token, proposal_from_value,
};
use serde_json::{Value, json};

fn proposal() -> RecoveryProposal {
    RecoveryProposal {
        state_root: "/tmp/state".to_string(),
        engine_version: "0.0.13+rust".to_string(),
        root_identity: FileIdentity { dev: 1, ino: 2 },
        lock_identity: Some(FileIdentity { dev: 1, ino: 3 }),
        inventory_sha256: "d".repeat(64),
        operation_id: "0123456789abcdef0123456789abcdef".to_string(),
        prior_journal_sha256: None,
    }
}

fn token_of(text: &str) -> String {
    URL_SAFE_NO_PAD.encode(text)
}

fn canonical_value() -> Value {
    serde_json::from_str(&encode_proposal(&proposal())).unwrap()
}

/// The token of `value` spelled in canonical form (sorted keys, compact), whatever it holds.
fn token_with(change: impl FnOnce(&mut Value)) -> String {
    let mut value = canonical_value();
    change(&mut value);
    token_of(&serde_json::to_string(&value).unwrap())
}

#[test]
fn a_state_root_of_every_kind_of_character_round_trips() {
    let mut proposal = proposal();
    proposal.state_root =
        "/\u{0}\u{1f}\"\\ \u{7f}\u{e9}\u{2028}\u{ffff}\u{10000}\u{1f600}\u{10ffff}".to_string();
    let token = encode_token(&proposal);
    assert!(token.is_ascii());
    assert_eq!(decode_token(&token), Ok(proposal));
}

#[test]
fn identities_at_the_extremes_round_trip() {
    let mut proposal = proposal();
    proposal.root_identity = FileIdentity {
        dev: i64::MIN,
        ino: u64::MAX,
    };
    proposal.lock_identity = Some(FileIdentity { dev: -1, ino: 0 });
    let text = encode_proposal(&proposal);
    assert!(text.contains("[-9223372036854775808,18446744073709551615]"));
    assert!(text.contains("[-1,0]"));
    assert_eq!(decode_token(&encode_token(&proposal)), Ok(proposal));
}

#[test]
fn a_proposal_encodes_with_sorted_keys_and_null_for_absent_values() {
    let mut proposal = proposal();
    proposal.lock_identity = None;
    let text = encode_proposal(&proposal);
    let keys: Vec<_> = serde_json::from_str::<serde_json::Map<String, Value>>(&text)
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        keys,
        [
            "engine_version",
            "inventory_sha256",
            "lock_identity",
            "operation_id",
            "prior_journal_sha256",
            "root_identity",
            "state_root",
        ]
    );
    assert!(
        text.contains(r#""lock_identity":null"#) && text.contains(r#""prior_journal_sha256":null"#)
    );
    assert!(!text.contains(' ') && !text.ends_with('\n'));
}

#[test]
fn the_token_bound_is_exact() {
    assert_eq!(TOKEN_BOUND, 8192);
    let sized = |length: usize| {
        let mut proposal = proposal();
        proposal.state_root = "r".repeat(length);
        proposal
    };
    let mut length = 4000;
    while encode_token(&sized(length + 1)).len() <= TOKEN_BOUND {
        length += 1;
    }
    let largest = sized(length);
    let token = encode_token(&largest);
    assert!(
        token.len() <= TOKEN_BOUND && token.len() >= TOKEN_BOUND - 2,
        "{}",
        token.len()
    );
    assert_eq!(decode_token(&token), Ok(largest));
    let over = encode_token(&sized(length + 1));
    assert!(over.len() > TOKEN_BOUND);
    assert_eq!(decode_token(&over), Err(StaleToken));
}

#[test]
fn rejected_tokens_are_stale() {
    let good = encode_token(&proposal());
    assert_eq!(decode_token(&good), Ok(proposal()));
    let stale = |label: &str, token: String| {
        assert_eq!(decode_token(&token), Err(StaleToken), "{label}");
    };
    stale("empty", String::new());
    stale("padded", format!("{good}="));
    stale("newline", format!("{good}\n"));
    stale("leading space", format!(" {good}"));
    stale("truncated", good[..good.len() - 4].to_string());
    stale("not an object: array", token_of("[]"));
    stale("not an object: null", token_of("null"));
    stale("not an object: number", token_of("1"));
    stale("not json", token_of("state_root"));
    stale("invalid utf-8", URL_SAFE_NO_PAD.encode([0xff, 0xfe]));
    stale("empty object", token_of("{}"));
    stale(
        "pretty",
        token_of(&serde_json::to_string_pretty(&canonical_value()).unwrap()),
    );
    stale(
        "trailing newline in the json",
        token_of(&format!("{}\n", encode_proposal(&proposal()))),
    );
    stale(
        "unsorted keys",
        token_of(&format!(
            r#"{{"state_root":"/tmp/state",{}"#,
            &encode_proposal(&proposal())[1..]
        )),
    );
    stale(
        "unescaped non-ascii",
        token_of(&encode_proposal(&proposal()).replace("/tmp/state", "/tmp/\u{e9}")),
    );
    stale("uppercase escape digits", {
        let mut proposal = proposal();
        proposal.state_root = "\u{e9}".to_string();
        token_of(&encode_proposal(&proposal).replace("\\u00e9", "\\u00E9"))
    });
    stale(
        "duplicate key",
        token_of(&encode_proposal(&proposal()).replace(
            r#""state_root":"/tmp/state""#,
            r#""state_root":"/x","state_root":"/tmp/state""#,
        )),
    );
}

#[test]
fn only_the_url_safe_alphabet_is_accepted() {
    // Roots whose JSON puts 0xfb..0xff-like six-bit groups ('>' and '?') in the token.
    let mut found = 0;
    for pad in 0..4 {
        let mut proposal = proposal();
        proposal.state_root = format!("/{}{}", "x".repeat(pad), "?>?>?>?>?>?>");
        let token = encode_token(&proposal);
        if !(token.contains('-') || token.contains('_')) {
            continue;
        }
        found += 1;
        assert_eq!(decode_token(&token), Ok(proposal));
        let standard = token.replace('-', "+").replace('_', "/");
        assert_eq!(decode_token(&standard), Err(StaleToken), "{standard}");
    }
    assert!(found > 0, "no sample used a url-safe character");
}

#[test]
fn a_token_whose_content_is_not_a_well_formed_proposal_is_stale() {
    let long = "g".repeat(32);
    let uppercase_id = "0123456789ABCDEF0123456789ABCDEF";
    let uppercase_digest = "D".repeat(64);
    let replaced: Vec<(&str, &str, Value)> = vec![
        ("short operation id", "operation_id", json!("0123")),
        (
            "uppercase operation id",
            "operation_id",
            json!(uppercase_id),
        ),
        ("non-hex operation id", "operation_id", json!(long)),
        ("operation id not text", "operation_id", json!(5)),
        ("short digest", "inventory_sha256", json!("ab")),
        (
            "uppercase digest",
            "inventory_sha256",
            json!(uppercase_digest),
        ),
        (
            "prior digest malformed",
            "prior_journal_sha256",
            json!("zz"),
        ),
        ("prior digest a number", "prior_journal_sha256", json!(1)),
        ("root identity one element", "root_identity", json!([1])),
        (
            "root identity three elements",
            "root_identity",
            json!([1, 2, 3]),
        ),
        ("root identity not an array", "root_identity", json!("1,2")),
        ("root identity null", "root_identity", json!(null)),
        ("root identity float", "root_identity", json!([1.5, 2])),
        (
            "root identity negative inode",
            "root_identity",
            json!([1, -2]),
        ),
        (
            "root identity device too large",
            "root_identity",
            json!([u64::MAX, 2]),
        ),
        ("lock identity a string", "lock_identity", json!("1,3")),
        ("lock identity short", "lock_identity", json!([1])),
        ("state root not text", "state_root", json!(["/tmp"])),
        ("engine version not text", "engine_version", json!(null)),
        ("extra field", "zz", json!(1)),
    ];
    for (label, key, value) in replaced {
        let token = token_with(|v| v[key] = value);
        assert_eq!(decode_token(&token), Err(StaleToken), "{label}");
    }
    let token = token_with(|v| {
        v.as_object_mut().unwrap().remove("state_root");
    });
    assert_eq!(decode_token(&token), Err(StaleToken), "missing field");
}

#[test]
fn a_token_with_null_lock_and_prior_digest_decodes() {
    let token = token_with(|v| {
        v["lock_identity"] = json!(null);
        v["prior_journal_sha256"] = json!("e".repeat(64));
    });
    let decoded = decode_token(&token).unwrap();
    assert_eq!(decoded.lock_identity, None);
    assert_eq!(decoded.prior_journal_sha256, Some("e".repeat(64)));
}

#[test]
fn a_token_for_another_engine_version_still_decodes() {
    // Binding to this engine is the recover step's business, not the codec's.
    let token = token_with(|v| v["engine_version"] = json!("0.0.14"));
    assert_eq!(decode_token(&token).unwrap().engine_version, "0.0.14");
}

#[test]
fn proposal_from_value_wants_exactly_seven_typed_fields() {
    assert_eq!(proposal_from_value(&canonical_value()), Some(proposal()));
    assert_eq!(proposal_from_value(&json!([])), None);
    assert_eq!(proposal_from_value(&json!(null)), None);
    let mut extra = canonical_value();
    extra["extra"] = json!(true);
    assert_eq!(proposal_from_value(&extra), None);
}
