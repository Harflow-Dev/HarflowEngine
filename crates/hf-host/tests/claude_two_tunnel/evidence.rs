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
pub(super) fn fake_capture(phase: &'static str, result: &actors::Captured) {
    let status = result.status;
    eprintln!(
        "phase={phase} original={} initialization={:?} exit-before-cleanup={:?} exit-after-cleanup={:?} observation-complete={} deadline-expired={} pump-error={} wait-error={} stdout-eof={} stderr-eof={} stdout-overflow={} stderr-overflow={} stdout-error-present={} stderr-error-present={} stdout-length={} stderr-length={}",
        result.identity,
        status.initialization,
        status.exit_before_cleanup,
        result.exit,
        result.complete,
        status.deadline_expired,
        status.pump_error,
        status.wait_error,
        status.stdout_eof,
        status.stderr_eof,
        status.stdout_overflow,
        status.stderr_overflow,
        status.stdout_error_present,
        status.stderr_error_present,
        result.stdout.len(),
        result.stderr.len()
    );
}
