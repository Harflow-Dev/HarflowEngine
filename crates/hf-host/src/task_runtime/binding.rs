//! Shared immutable launch bindings, independently checked in dispatcher and owner.
use hf_application::owner_runtime::DigestPort;
use hf_domain::{ownership::Digest, task_launch::CliSessionId};
pub fn configuration_digest() -> Digest {
    hf_storage::Sha256Digest.digest_parts(&[b"first-public-task-v1", b"claude", b"unrestricted"])
}
pub fn invocation_arguments(session: &CliSessionId) -> Vec<String> {
    [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--permission-mode",
        "bypassPermissions",
        "--input-format",
        "stream-json",
        "--session-id",
        session.as_str(),
    ]
    .iter()
    .map(|s| (*s).into())
    .collect()
}
pub fn invocation_digest(session: &CliSessionId) -> Digest {
    let args = invocation_arguments(session);
    let parts: Vec<_> = args.iter().map(|s| s.as_bytes()).collect();
    hf_storage::Sha256Digest.digest_parts(&parts)
}
pub fn timestamp() -> Result<String, super::Error> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| super::Error::Refused)?;
    let seconds = elapsed.as_secs();
    let days = i64::try_from(seconds / 86400).map_err(|_| super::Error::Refused)?;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    if !(1970..=9999).contains(&year) {
        return Err(super::Error::Refused);
    }
    let day_seconds = seconds % 86400;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:06}+00:00",
        day_seconds / 3600,
        day_seconds % 3600 / 60,
        day_seconds % 60,
        elapsed.subsec_micros()
    ))
}
pub(super) fn source_binding(
    raw: &hf_platform::task_runtime::raw::RawCapture,
    v: &hf_domain::task_runtime::ProductionInvitation,
) -> Digest {
    let (out, err) = raw.source_identities();
    hf_storage::Sha256Digest.digest_parts(&[
        v.binding.task.as_str().as_bytes(),
        v.binding.operation.as_str().as_bytes(),
        v.binding.attempt.as_str().as_bytes(),
        v.binding.generation.as_str().as_bytes(),
        &out.device.to_be_bytes(),
        &out.inode.to_be_bytes(),
        &err.device.to_be_bytes(),
        &err.inode.to_be_bytes(),
    ])
}
