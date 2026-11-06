use crate::qualification::quote_sbpl;
use std::io;
use std::path::Path;

/// A separate profile: no workspace/runtime write grants and no env wrapper.
/// Rendering is diagnostic preparation, never evidence that Seatbelt enforced it.
pub fn absence_profile(helper: &Path) -> io::Result<String> {
    if !helper.is_absolute() || std::fs::canonicalize(helper)? != helper {
        return Err(io::Error::other("physical absolute helper required"));
    }
    let meta = std::fs::symlink_metadata(helper)?;
    if !meta.is_file() || meta.len() > 1_048_576 {
        return Err(io::Error::other("bounded regular helper required"));
    }
    let path = quote_sbpl(
        helper
            .to_str()
            .ok_or_else(|| io::Error::other("UTF8 helper"))?,
    )?;
    Ok(format!(
        "(version 1)\n(deny default)\n(allow process-exec (literal {path}))\n\
         (allow file-read-metadata)\n(allow file-read-data (literal \"/\") (literal {path})\n\
         (subpath \"/System/Library\") (subpath \"/usr/lib\"))\n\
         (deny file-write* process-fork network* signal mach-lookup appleevent-send)\n"
    ))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeControl {
    None,
    Write,
    Read,
    Fork,
    Network,
    Signal,
}

pub(super) fn controlled_profile(
    helper: &Path,
    root: &Path,
    port: u16,
    control: ProbeControl,
) -> io::Result<String> {
    let mut profile = absence_profile(helper)?;
    let (deny, grant) = match control {
        ProbeControl::None => return Ok(profile),
        ProbeControl::Write => (
            "file-write* ",
            format!(
                "(allow file-write* (literal {}))\n",
                quote_sbpl(
                    root.join("workspace/st04-effect")
                        .to_str()
                        .ok_or_else(|| io::Error::other("UTF8 root"))?
                )?
            ),
        ),
        ProbeControl::Read => (
            "",
            format!(
                "(allow file-read-data (literal {}))\n",
                quote_sbpl(
                    root.join("workspace/st04-read-witness")
                        .to_str()
                        .ok_or_else(|| io::Error::other("UTF8 root"))?
                )?
            ),
        ),
        ProbeControl::Fork => ("process-fork ", "(allow process-fork)\n".into()),
        ProbeControl::Network => (
            "network* ",
            format!("(allow network-outbound (remote ip \"localhost:{port}\"))\n"),
        ),
        ProbeControl::Signal => ("signal ", "(allow signal)\n".into()),
    };
    if !deny.is_empty() {
        profile = profile.replace(deny, "");
    }
    profile.push_str(&grant);
    Ok(profile)
}
