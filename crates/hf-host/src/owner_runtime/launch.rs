//! HFPL1 checks remain denial witnesses, never arbitrary executable admission.
use hf_domain::{fixture::Prepared, prepared_launch::PreparedLaunch, recovery::SourceStamp};
use hf_interfaces::prepared_launch::{self, DecodedPreparation};
use hf_storage::Sha256Digest;
use std::{path::Path, time::Instant};
pub(super) fn decode(p: &Prepared) -> Result<Option<PreparedLaunch>, String> {
    match prepared_launch::decode_preparation(&p.request, &Sha256Digest).map_err(error)? {
        DecodedPreparation::Legacy(_) => Ok(None),
        DecodedPreparation::FixedProtocol(value) => Ok(Some(*value)),
    }
}
pub(super) fn check(
    p: &Prepared,
    source: Option<&SourceStamp>,
    until: Instant,
) -> Result<Option<PreparedLaunch>, String> {
    let Some(value) = decode(p)? else {
        return Ok(None);
    };
    let live =
        hf_storage::RootHold::open(&p.root, until, hf_platform::lease::shared()).map_err(error)?;
    if live.source_stamp() != &value.source {
        return Err("prepared root source changed".into());
    }
    let current = std::env::current_exe().map_err(error)?;
    let pin = hf_platform::prepared_launch::observe_binary(&current, &Sha256Digest, until)
        .map_err(error)?;
    let cwd = std::env::current_dir().map_err(error)?;
    if pin != value.binary
        || cwd != Path::new(&value.cwd)
        || std::fs::canonicalize(&cwd).map_err(error)? != cwd
        || Path::new(&p.root).parent() != Some(cwd.as_path())
        || p.ledger != value.ledger
        || source.is_some_and(|s| s != &value.source)
        || hf_platform::fixture::qualify_ledger(Path::new(&p.root)).map_err(error)? != value.ledger
    {
        return Err("prepared executable/source changed".into());
    }
    Ok(Some(value))
}
fn error(e: impl std::fmt::Debug) -> String {
    format!("prepared launch: {e:?}")
}
