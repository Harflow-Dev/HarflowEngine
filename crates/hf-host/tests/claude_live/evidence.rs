//! Metadata only. Real-result entry is called only after token checks and scan.
use super::{Attempt, actors};
pub(super) fn instance(attempt: &Attempt) {
    let mut digest = hf_storage::Sha256Stream::with_limit(32768).unwrap();
    digest.update(attempt.profile.as_bytes()).unwrap();
    let profile = digest.finish().unwrap();
    eprintln!(
        "phase=instance root={} nonce={} profile-sha256={} loopback-port={}",
        attempt.root.root().display(),
        attempt.nonce,
        profile.as_str(),
        attempt.proxy.as_ref().unwrap().port()
    );
}
pub(super) fn version(attempt: &Attempt) {
    eprintln!(
        "phase=preflight-version root={} nonce={} controlled-spawn=passed no-fork-no-spawn-read-network-protected-write=passed exact-version=2.1.318 vendor-length=319598464 vendor-sha256=677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d icu-length=35444880 icu-sha256=666a97d4dd1139f537290d74a661cc6b6da6cc58da24486b3cb282eacbcfa549 bookends=passed",
        attempt.root.root().display(),
        attempt.nonce
    );
}
pub(super) fn real(
    result: &actors::Captured,
    outcome: &hf_platform::qualification::live::ProxyOutcome,
) {
    eprintln!(
        "phase=real-original identity={} exit={:?} stdout-length={} stderr-length={} both-pipe-eof-clean={} literal-output=absent owned-root-secret-matches=0 ordered-nonce-success=passed proxy={:?} cost=not-observed",
        result.identity,
        result.exit,
        result.stdout.len(),
        result.stderr.len(),
        result.complete,
        outcome
    );
}
