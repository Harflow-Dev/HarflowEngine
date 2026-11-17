//! Bounded explicit provider environment; the same-release owner/writer themselves start empty.
use super::invitation::{Reader, number, text};
use hf_application::task_runtime::InvalidIntent;
pub fn encode_environment(values: &[(String, String)]) -> Result<Vec<u8>, InvalidIntent> {
    if values.len() > 128 {
        return Err(InvalidIntent);
    }
    let mut out = b"HFTE\x01".to_vec();
    number(&mut out, values.len() as u64);
    let mut keys = Vec::new();
    for (key, value) in values {
        if key.is_empty() || key.contains('=') || keys.contains(key) {
            return Err(InvalidIntent);
        }
        keys.push(key.clone());
        text(&mut out, key)?;
        text(&mut out, value)?;
    }
    if out.len() > 65536 {
        return Err(InvalidIntent);
    }
    Ok(out)
}
pub fn decode_environment(bytes: &[u8]) -> Result<Vec<(String, String)>, InvalidIntent> {
    if bytes.len() > 65536 {
        return Err(InvalidIntent);
    }
    let mut r = Reader { bytes, at: 0 };
    if r.take(5)? != b"HFTE\x01" {
        return Err(InvalidIntent);
    }
    let count = r.number()?;
    if count > 128 {
        return Err(InvalidIntent);
    }
    let mut values = Vec::new();
    for _ in 0..count {
        values.push((r.text()?, r.text()?));
    }
    if r.at != bytes.len() || encode_environment(&values)? != bytes {
        return Err(InvalidIntent);
    }
    Ok(values)
}
