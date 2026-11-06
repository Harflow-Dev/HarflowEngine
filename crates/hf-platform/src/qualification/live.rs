//! Private direct-child diagnostic effects; no production admission.
mod broker;
mod proxy;
mod scan;
use super::{FixtureRoot, recorded_vendor_profile};
pub use broker::read_credential_record;
pub use proxy::{
    ExtraAcceptSnapshot, Proxy, ProxyFailure, ProxyFailureStage, ProxyMode, ProxyOutcome,
    RejectionCounts, RelayDirectionSnapshot,
};
pub use scan::{OriginalActorsSettled, SecretMatches, remove_matched_files, scan_secret};
use std::io;
/// Same recorded closure plus a denial, never an additional grant.
pub fn live_profile(root: &FixtureRoot, port: u16) -> io::Result<String> {
    let mut profile = recorded_vendor_profile(&root.recorded_topology()?, port)?;
    profile.push_str("(deny file-write* (regex #\"(^|/)[.]credentials[.]json$\"))\n");
    Ok(profile)
}

mod two_tunnel;
pub use two_tunnel::*;
