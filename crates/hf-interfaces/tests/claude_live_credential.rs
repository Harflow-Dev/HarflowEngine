#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_interfaces::claude_live_credential::{CredentialError as E, parse_oauth_access};

const NOW: u64 = 1_000_000;
const MIN: u64 = NOW + 130_000;
const TOKEN: &str = "opaque-live-access-secret";

fn record(access: &str, expires: u64, scopes: &str) -> Vec<u8> {
    format!(
        r#"{{"claudeAiOauth":{{"accessToken":"{access}","refreshToken":"must-not-escape","expiresAt":{expires},"scopes":{scopes}}},"other":{{"accessToken":"unrelated"}}}}"#
    )
    .into_bytes()
}

#[test]
fn parser_returns_only_volatile_access_and_supports_literal_scan() {
    let parsed =
        parse_oauth_access(&record(TOKEN, MIN, r#"["user:inference"]"#), NOW, MIN).unwrap();
    let (name, value) = parsed.environment_pair();
    assert_eq!(name, "CLAUDE_CODE_OAUTH_TOKEN");
    assert_eq!(value, TOKEN);
    assert!(parsed.contains_in(format!("prefix {TOKEN} suffix").as_bytes()));
    assert!(!parsed.contains_in(b"refreshToken must-not-escape"));
}

#[test]
fn parser_rejects_malformed_expired_or_inadequately_scoped_records() {
    assert!(matches!(
        parse_oauth_access(b"{}", NOW, MIN),
        Err(E::MissingOauth)
    ));
    assert!(matches!(
        parse_oauth_access(b"not-json", NOW, MIN),
        Err(E::Malformed)
    ));
    assert!(matches!(
        parse_oauth_access(&record(TOKEN, MIN - 1, r#"["user:inference"]"#), NOW, MIN),
        Err(E::InvalidExpiry)
    ));
    assert!(matches!(
        parse_oauth_access(&record(TOKEN, MIN, r#"["user:profile"]"#), NOW, MIN),
        Err(E::MissingInferenceScope)
    ));
    assert!(matches!(
        parse_oauth_access(&record("", MIN, r#"["user:inference"]"#), NOW, MIN),
        Err(E::InvalidAccess)
    ));
    assert!(matches!(
        parse_oauth_access(&record(TOKEN, MIN, r#"["user:inference",1]"#), NOW, MIN),
        Err(E::MissingInferenceScope)
    ));
    assert!(matches!(
        parse_oauth_access(
            &record("token with space", MIN, r#"["user:inference"]"#),
            NOW,
            MIN
        ),
        Err(E::InvalidAccess)
    ));
}

#[test]
fn parser_rejects_invalid_clock_and_oversized_input_with_static_errors() {
    assert!(matches!(
        parse_oauth_access(b"{}", u64::MAX, u64::MAX),
        Err(E::InvalidClock)
    ));
    assert!(matches!(
        parse_oauth_access(b"{}", NOW, MIN - 1),
        Err(E::InvalidClock)
    ));
    assert!(matches!(
        parse_oauth_access(&vec![b' '; 65_537], NOW, MIN),
        Err(E::Oversized)
    ));
}

#[test]
fn parser_rejects_duplicate_keys_even_when_the_second_spelling_is_escaped() {
    for raw in [
        br#"{"claudeAiOauth":{"accessToken":"first","accessToken":"second","expiresAt":1130000,"scopes":["user:inference"]}}"#.as_slice(),
        br#"{"claudeAiOauth":{"accessToken":"first","access\u0054oken":"second","expiresAt":1130000,"scopes":["user:inference"]}}"#.as_slice(),
        br#"{"claudeAiOauth":{"accessToken":"first","expiresAt":1130000,"expiresAt":1130001,"scopes":["user:inference"]}}"#.as_slice(),
    ] {
        assert!(matches!(
            parse_oauth_access(raw, NOW, MIN),
            Err(E::Malformed)
        ));
    }
}
