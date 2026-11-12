#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The wire cursor of `task-pending-page`, read as the reference reads it: strict base64,
//! the size bound, and the errors of a cursor that no longer matches the store.
mod ctl_reads_support;
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD},
};
use ctl_reads_support::{args, pending_cursor::*};
use hf_application::ctl::task_reads::Namespace;
use hf_domain::ctl::pending::LaunchFacts;
use hf_interfaces::ctl::dispatch::dispatch;
use serde_json::Value;

#[test]
fn a_well_formed_cursor_resumes_the_walk() {
    assert!(accepted(&world(), &url(&good())));
}

#[test]
fn member_order_and_whitespace_do_not_matter() {
    let shuffled = format!(
        " {{ \"v\" : 1 , \"uncertain\":false,\"scope\":\"{SCOPE}\", \"root\":\"{}\",\"revision\":\"{}\",\"limit\":2,\"last\":\"{FIRST}\",\"identity\":[ 1, 2,1 ,3]}} ",
        "f".repeat(64),
        "a".repeat(64)
    );
    assert!(accepted(&world(), &url(&shuffled)));
}

#[test]
fn both_base64_alphabets_are_accepted() {
    // A UTF-8 byte order mark, which Python's `json.loads` of bytes skips, is the one way a
    // cursor body of valid JSON reaches the characters '+' and '/' (or '-' and '_').
    let text = format!("\u{feff}{}", good());
    let urlsafe = URL_SAFE.encode(&text);
    let standard = STANDARD.encode(&text);
    assert_ne!(urlsafe, standard);
    assert!(standard.contains(['+', '/']) && urlsafe.contains(['-', '_']));
    assert!(accepted(&world(), &standard), "standard alphabet");
    assert!(accepted(&world(), &urlsafe), "url-safe alphabet");
}

#[test]
fn padding_must_be_complete_and_exact() {
    let mut checked = 0;
    for pad in 0..3 {
        let text = format!("{}{}", good(), " ".repeat(pad));
        let padded = URL_SAFE.encode(&text);
        let bare = URL_SAFE_NO_PAD.encode(&text);
        if padded.ends_with('=') {
            checked += 1;
            assert!(accepted(&world(), &padded));
            assert!(!accepted(&world(), &bare), "missing padding");
            assert!(!accepted(&world(), &format!("{padded}=")), "excess padding");
            assert!(!accepted(&world(), &format!("{padded}===")), "three pads");
            let one_short = &padded[..padded.len() - 1];
            assert!(!accepted(&world(), one_short), "incomplete padding");
        }
    }
    assert!(checked > 0);
}

#[test]
fn trailing_bits_in_the_last_character_are_ignored() {
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    for pad in 0..3 {
        let text = format!("{}{}", good(), " ".repeat(pad));
        if text.len() % 3 != 1 {
            continue;
        }
        // One tail byte: two characters, the second carries four unused bits.
        let canonical = URL_SAFE.encode(&text);
        let data = canonical.trim_end_matches('=');
        let last = data.chars().last().unwrap();
        let index = ALPHABET.find(last).unwrap();
        let other = ALPHABET.chars().nth((index & !0xf) | 0xf).unwrap();
        let mut changed = data[..data.len() - 1].to_owned();
        changed.push(other);
        changed.push_str("==");
        assert!(accepted(&world(), &changed), "{changed}");
        return;
    }
    panic!("no body length with a one-byte tail");
}

#[test]
fn text_that_is_not_ascii_or_not_base64_is_malformed() {
    let fake = world();
    for raw in [
        "é".to_owned(),
        format!("{}é", url(&good())),
        "not base64!".to_owned(),
        format!("{}\n", url(&good())),
        format!(" {}", url(&good())),
        url(&good()).replace('=', "\u{ff1d}"),
    ] {
        assert!(!accepted(&fake, &raw), "{raw:?}");
    }
    assert!(!accepted(&fake, "e30="), "an empty object");
    assert!(!accepted(&fake, &url("[]")));
    assert!(!accepted(&fake, &url("1")));
    assert!(!accepted(&fake, &url("null")));
    assert!(!accepted(&fake, &URL_SAFE.encode([0xff, 0xfe, b'{'])));
}

#[test]
fn the_cursor_is_at_most_4096_bytes() {
    let fake = world();
    let at = (0..3200)
        .map(|pad| URL_SAFE.encode(format!("{}{}", good(), " ".repeat(pad))))
        .find(|encoded| encoded.len() == 4096)
        .unwrap();
    assert!(accepted(&fake, &at));
    let over = (0..3300)
        .map(|pad| URL_SAFE.encode(format!("{}{}", good(), " ".repeat(pad))))
        .find(|encoded| encoded.len() == 4100)
        .unwrap();
    assert!(!accepted(&fake, &over));
}

#[test]
fn a_cursor_of_another_root_or_limit_is_a_scope_error() {
    let fake = world();
    let other_root = replaced(&"f".repeat(64), &"e".repeat(64));
    assert_eq!(
        error(&page(&fake, &other_root, "2")),
        ("pending_cursor_invalid".into(), "cursor_scope".into())
    );
    assert_eq!(
        error(&page(&fake, &url(&good()), "3")),
        ("pending_cursor_invalid".into(), "cursor_scope".into())
    );
    let default_limit = dispatch(
        &args(&["task-pending-page", "--cursor", &url(&good()), "--json"]),
        &fake,
    );
    assert_eq!(
        error(&default_limit),
        ("pending_cursor_invalid".into(), "cursor_scope".into()),
        "the default limit 50 is not the cursor's"
    );
}

#[test]
fn a_changed_namespace_or_store_expires_the_cursor_and_an_unknown_position_is_refused() {
    let mut fake = world();
    let expired = (
        "pending_cursor_expired".to_owned(),
        "namespace_changed".to_owned(),
    );
    fake.pending.namespace = Ok(Namespace {
        revision: "b".repeat(64),
        invalid: 0,
    });
    assert_eq!(error(&page(&fake, &url(&good()), "2")), expired);
    let mut fake = world();
    fake.pending.binding.identity = [1, 2, 1, 4];
    assert_eq!(error(&page(&fake, &url(&good()), "2")), expired);
    let mut fake = world();
    fake.pending.rows.remove(0);
    assert_eq!(
        error(&page(&fake, &url(&good()), "2")),
        ("pending_cursor_invalid".into(), "cursor_position".into())
    );
}

#[test]
fn a_cursor_never_carries_across_an_uncertain_flag_it_did_not_have() {
    let mut fake = world();
    fake.pending.rows[0] = LaunchFacts {
        id: FIRST.into(),
        refusal: Some(("format".into(), "format".into())),
        ..LaunchFacts::default()
    };
    fake.pending
        .rows
        .push(row("cccccccccccccccccccccccccccccccc"));
    let out = dispatch(
        &args(&["task-pending-page", "--limit", "2", "--json"]),
        &fake,
    );
    let value: Value = serde_json::from_str(&out.stdout).unwrap();
    let next = value["result"]["next_cursor"].as_str().unwrap();
    let decoded: Value = serde_json::from_slice(&URL_SAFE.decode(next).unwrap()).unwrap();
    assert_eq!(decoded["uncertain"], Value::Bool(true));
    assert_eq!(decoded["last"], SECOND);
    assert_eq!(decoded["scope"], SCOPE);
    assert_eq!(decoded["limit"], 2);
    assert_eq!(decoded["identity"], serde_json::json!([1, 2, 1, 3]));
    assert_eq!(decoded["v"], 1);
    assert_eq!(decoded.as_object().unwrap().len(), 8);
}
