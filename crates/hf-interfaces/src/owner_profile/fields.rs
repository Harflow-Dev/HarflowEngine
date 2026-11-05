use super::{Checked, invalid, unsupported};
use hf_domain::{
    ownership::{Digest, Id},
    process::ProcessIdentity,
};
use serde_json::Value;
pub(super) fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
pub(super) fn token(v: &Value, len: usize) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == len
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
pub(super) fn integer(v: &Value, low: u64, high: u64) -> bool {
    v.as_u64().is_some_and(|n| n >= low && n <= high)
}
pub(super) fn id(v: &Value) -> Checked<Id> {
    if !token(v, 32) {
        return Err(invalid("fields"));
    }
    Id::new(v.as_str().unwrap_or("").to_owned()).map_err(|_| invalid("fields"))
}
pub(super) fn digest(v: &Value) -> Checked<Digest> {
    if !token(v, 64) {
        return Err(invalid("fields"));
    }
    Digest::new(v.as_str().unwrap_or("").to_owned()).map_err(|_| invalid("fields"))
}
pub(super) fn process(v: &Value) -> Option<ProcessIdentity> {
    let boot = v["boot_uuid"].as_str()?;
    if !v.is_object()
        || !integer(&v["pid"], 2, 2147483647)
        || !integer(&v["start_seconds"], 1, u64::MAX)
        || !integer(&v["start_microseconds"], 0, 999999)
        || boot.len() != 36
        || !boot.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
    {
        return None;
    }
    ProcessIdentity::new(
        v["pid"].as_u64()? as u32,
        v["start_seconds"].as_u64()?,
        v["start_microseconds"].as_u64()?,
        boot.to_owned(),
    )
    .ok()
}
pub(super) fn shape(d: &Value, strings: &[&str], booleans: &[&str]) -> Checked<()> {
    if !strings.iter().all(|k| d[k].is_string()) || !booleans.iter().all(|k| d[k].is_boolean()) {
        return Err(invalid("fields"));
    }
    Ok(())
}
pub(super) fn enums(d: &Value, values: &[(&str, &str)]) -> Checked<()> {
    if values
        .iter()
        .any(|(key, values)| !values.split_whitespace().any(|s| s == text(d, key)))
    {
        return Err(unsupported("enum"));
    }
    Ok(())
}
