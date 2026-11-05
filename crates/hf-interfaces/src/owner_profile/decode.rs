use super::{Checked, invalid};
use serde_json::Value;
use std::collections::BTreeSet;
pub(super) fn decode(raw: &[u8]) -> Checked<Value> {
    if raw.len() > 24576 {
        return Err(invalid("budget"));
    }
    std::str::from_utf8(raw).map_err(|_| invalid("encoding"))?;
    lexical_depth(raw)?;
    duplicates(raw)?;
    let normalized = integer_zero(raw);
    let value: Value = serde_json::from_slice(normalized.as_deref().unwrap_or(raw))
        .map_err(|_| invalid("encoding"))?;
    budgets(&value, 0)?;
    Ok(value)
}
fn lexical_depth(raw: &[u8]) -> Checked<()> {
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for b in raw {
        if quoted {
            if escaped {
                escaped = false;
            } else if *b == b'\\' {
                escaped = true;
            } else if *b == b'"' {
                quoted = false;
            }
            continue;
        }
        if b" \t\r\n,:".contains(b) {
            continue;
        }
        if b"]}".contains(b) {
            depth = depth.saturating_sub(1);
            continue;
        }
        if depth > 16 {
            return Err(invalid("budget"));
        }
        if *b == b'"' {
            quoted = true;
        } else if b"[{".contains(b) {
            depth += 1;
        }
    }
    Ok(())
}
/// Decode keys independently so escaped spellings cannot bypass duplicate rejection.
fn duplicates(raw: &[u8]) -> Checked<()> {
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
                    return Err(invalid("encoding"));
                }
                let end = at + 1;
                let mut next = end;
                while next < raw.len() && raw[next].is_ascii_whitespace() {
                    next += 1;
                }
                if raw.get(next) == Some(&b':') {
                    let key: String = serde_json::from_slice(&raw[start..end])
                        .map_err(|_| invalid("encoding"))?;
                    if let Some(Some(keys)) = stack.last_mut()
                        && !keys.insert(key)
                    {
                        return Err(invalid("encoding"));
                    }
                }
            }
            _ => (),
        }
        at += 1;
    }
    Ok(())
}
fn budgets(v: &Value, depth: usize) -> Checked<()> {
    if depth > 16 {
        return Err(invalid("budget"));
    }
    match v {
        Value::String(s) if s.len() > 4096 => return Err(invalid("budget")),
        Value::Array(a) => {
            if a.len() > 64 {
                return Err(invalid("budget"));
            }
            for item in a {
                budgets(item, depth + 1)?;
            }
        }
        Value::Object(o) => {
            for (key, item) in o {
                if depth + 1 > 16 || key.len() > 4096 {
                    return Err(invalid("budget"));
                }
                budgets(item, depth + 1)?;
            }
        }
        _ => (),
    }
    Ok(())
}

/// JSON's integer spelling -0 is zero; preserve its integer type (unlike -0.0).
fn integer_zero(raw: &[u8]) -> Option<Vec<u8>> {
    let (mut quoted, mut escaped) = (false, false);
    let mut normalized: Option<Vec<u8>> = None;
    for (at, b) in raw.iter().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if *b == b'\\' {
                escaped = true;
            } else if *b == b'"' {
                quoted = false;
            }
            continue;
        }
        if *b == b'"' {
            quoted = true;
            continue;
        }
        let preceding = at == 0 || b" \t\r\n[:,".contains(&raw[at - 1]);
        let following =
            at + 2 == raw.len() || raw.get(at + 2).is_some_and(|b| b" \t\r\n,]}".contains(b));
        if *b == b'-' && raw.get(at + 1) == Some(&b'0') && preceding && following {
            normalized.get_or_insert_with(|| raw.to_vec())[at] = b' ';
        }
    }
    normalized
}
