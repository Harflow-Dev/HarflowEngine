//! Closed prepared routes remain diagnostic, never production admission.
use hf_domain::{fixture::Prepared, recovery::SourceStamp};
use hf_interfaces::prepared_launch::{self, DecodedPreparation};
use hf_storage::Sha256Digest;
use std::{path::Path, time::Instant};
pub(super) fn decode(p: &Prepared) -> Result<Option<DecodedPreparation>, String> {
    match prepared_launch::decode_preparation(&p.request, &Sha256Digest).map_err(error)? {
        DecodedPreparation::Legacy(_) => Ok(None),
        value => Ok(Some(value)),
    }
}
pub(super) fn check(
    p: &Prepared,
    source: Option<&SourceStamp>,
    until: Instant,
) -> Result<Option<DecodedPreparation>, String> {
    let Some(value) = decode(p)? else {
        return Ok(None);
    };
    let (expected_source, expected_binary, cwd, ledger) = match &value {
        DecodedPreparation::FixedProtocol(v) => (&v.source, &v.binary, &v.cwd, v.ledger),
        DecodedPreparation::ClaudeQualification(v) => {
            crate::fixture::claude_bridge::validate(v, until).map_err(error)?;
            (&v.source, &v.gate_binary, &v.cwd, v.ledger)
        }
        DecodedPreparation::Legacy(_) => return Err("legacy prepared route".into()),
    };
    let live =
        hf_storage::RootHold::open(&p.root, until, hf_platform::lease::shared()).map_err(error)?;
    let pin = hf_platform::prepared_launch::observe_binary(
        &std::env::current_exe().map_err(error)?,
        &Sha256Digest,
        until,
    )
    .map_err(error)?;
    let actual_cwd = std::env::current_dir().map_err(error)?;
    if live.source_stamp() != expected_source
        || &pin != expected_binary
        || actual_cwd != Path::new(cwd)
        || std::fs::canonicalize(&actual_cwd).map_err(error)? != actual_cwd
        || Path::new(&p.root).parent() != Some(actual_cwd.as_path())
        || p.ledger != ledger
        || source.is_some_and(|s| s != expected_source)
        || hf_platform::fixture::qualify_ledger(Path::new(&p.root)).map_err(error)? != ledger
    {
        return Err("prepared executable/source changed".into());
    }
    Ok(Some(value))
}
fn error(e: impl std::fmt::Debug) -> String {
    format!("prepared launch: {e:?}")
}
