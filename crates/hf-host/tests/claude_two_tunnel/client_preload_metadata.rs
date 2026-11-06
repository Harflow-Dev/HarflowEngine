use hf_domain::process::ProcessIdentity;
/// Exact single fixed row only; malformed input never becomes diagnostic content.
pub fn parse_ready(
    bytes: &[u8],
    nonce: &str,
    identity: &ProcessIdentity,
) -> Result<(), &'static str> {
    if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) || bytes.len() > 512 {
        return Err("unavailable");
    }
    let expected = format!("CLIENT_PRELOAD_READY_V1\t{nonce}\t{}\n", identity.pid());
    if bytes == expected.as_bytes() {
        Ok(())
    } else {
        Err("unavailable")
    }
}
