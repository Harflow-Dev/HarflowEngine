//! Bounded in-memory decoding of an account-bound credential record.
//! No zeroization or universal vendor nonpersistence guarantee is provided.
use serde_json::Value;
use std::collections::BTreeSet;

const MAX_RECORD_BYTES: usize = 64 * 1024;
const MIN_LIFETIME_MS: u64 = 130_000;

/// Intentionally has no Debug, Clone, or serialization implementation.
pub struct OAuthAccess {
    access: String,
}

impl OAuthAccess {
    /// For a volatile, cleared child environment only; never log this pair.
    pub fn environment_pair(&self) -> (&'static str, &str) {
        ("CLAUDE_CODE_OAUTH_TOKEN", &self.access)
    }

    /// Literal leak detection only; does not serialize or digest the token.
    pub fn contains_in(&self, bytes: &[u8]) -> bool {
        bytes
            .windows(self.access.len())
            .any(|v| v == self.access.as_bytes())
    }
}

/// Static failures deliberately contain no credential input or parser details.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialError {
    Oversized,
    Malformed,
    InvalidClock,
    MissingOauth,
    InvalidAccess,
    InvalidExpiry,
    MissingInferenceScope,
}

/// Only the access token is returned. Refresh tokens and unrelated MCP OAuth
/// records are ignored. The host supplies both clock values before launch.
pub fn parse_oauth_access(
    raw: &[u8],
    now_ms: u64,
    min_valid_until_ms: u64,
) -> Result<OAuthAccess, CredentialError> {
    if raw.len() > MAX_RECORD_BYTES {
        return Err(CredentialError::Oversized);
    }
    let earliest = now_ms
        .checked_add(MIN_LIFETIME_MS)
        .ok_or(CredentialError::InvalidClock)?;
    if min_valid_until_ms < earliest {
        return Err(CredentialError::InvalidClock);
    }
    reject_duplicates(raw)?;
    let record: Value = serde_json::from_slice(raw).map_err(|_| CredentialError::Malformed)?;
    let oauth = record
        .as_object()
        .and_then(|v| v.get("claudeAiOauth"))
        .and_then(Value::as_object)
        .ok_or(CredentialError::MissingOauth)?;
    let access = oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 4096 && s.bytes().all(|b| b.is_ascii_graphic()))
        .ok_or(CredentialError::InvalidAccess)?;
    let expiry = oauth
        .get("expiresAt")
        .and_then(Value::as_u64)
        .ok_or(CredentialError::InvalidExpiry)?;
    if expiry < min_valid_until_ms {
        return Err(CredentialError::InvalidExpiry);
    }
    let scopes = oauth
        .get("scopes")
        .and_then(Value::as_array)
        .ok_or(CredentialError::MissingInferenceScope)?;
    if !scopes.iter().all(Value::is_string)
        || !scopes.iter().any(|v| v.as_str() == Some("user:inference"))
    {
        return Err(CredentialError::MissingInferenceScope);
    }
    Ok(OAuthAccess {
        access: access.into(),
    })
}

/// Decode keys independently so escaped spellings cannot bypass duplicate rejection.
fn reject_duplicates(raw: &[u8]) -> Result<(), CredentialError> {
    let mut stack: Vec<Option<BTreeSet<String>>> = Vec::new();
    let mut at = 0;
    while at < raw.len() {
        match raw[at] {
            b'{' => stack.push(Some(BTreeSet::new())),
            b'[' => stack.push(None),
            b'}' | b']' => {
                stack.pop();
            }
            b'"' => {
                let start = at;
                at += 1;
                while at < raw.len() && raw[at] != b'"' {
                    if raw[at] == b'\\' {
                        at += 1;
                    }
                    at += 1;
                }
                if at >= raw.len() {
                    return Err(CredentialError::Malformed);
                }
                let end = at + 1;
                let mut next = end;
                while next < raw.len() && raw[next].is_ascii_whitespace() {
                    next += 1;
                }
                if raw.get(next) == Some(&b':') {
                    let key: String = serde_json::from_slice(&raw[start..end])
                        .map_err(|_| CredentialError::Malformed)?;
                    if let Some(Some(keys)) = stack.last_mut()
                        && !keys.insert(key)
                    {
                        return Err(CredentialError::Malformed);
                    }
                }
            }
            _ => (),
        }
        at += 1;
    }
    Ok(())
}
