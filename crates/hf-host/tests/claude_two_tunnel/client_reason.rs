use super::client_reason_sources::Sources;
use super::*;
use hf_domain::process::ProcessExit;
const FAKE: &[u8] = b"harflow-invalid-negative-token";
pub(super) fn caught<T>(f: impl FnOnce() -> T) -> std::thread::Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
}
pub(super) fn run() {
    assert!(
        env::var(Mode::ClientReason.gate()).as_deref() == Ok("1"),
        "explicit fake-only gate required"
    );
    let overall = Instant::now() + Duration::from_secs(130);
    let selected = identity::selection();
    assert!(
        selected.expected_hash
            == "677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d",
        "exact vendor pin"
    );
    let root = FixtureRoot::create().unwrap();
    prepare_dirs(&root);
    let sources = Sources::create(&root);
    let helper = tls::start(
        &root,
        overall.min(Instant::now() + Duration::from_secs(30)),
        Mode::ClientReason,
    );
    let address = helper.address;
    let attempt = Attempt::from_root(root, address, overall, selected, Mode::ClientReason);
    let mut before = None;
    let mut version = None;
    let mut negative = None;
    let execution = caught(|| {
        attempt.preflight_only(overall);
        let until = overall.min(Instant::now() + Duration::from_secs(15));
        before = Some(pins::observe(until));
        version = Some(invoke(&attempt, &sources, true, until));
        assert!(
            version
                .as_ref()
                .is_some_and(|v| version_valid(v, &sources, &attempt)),
            "observer version unavailable"
        );
        pins::unchanged(before.as_ref().unwrap(), until);
        let until = overall.min(Instant::now() + Duration::from_secs(15));
        negative = Some(invoke(&attempt, &sources, false, until));
        await_proxy(attempt.proxy.as_ref().unwrap(), until)
    });
    settle(
        attempt,
        sources,
        helper,
        before,
        (version, negative),
        execution,
        overall,
    );
}
fn settle(
    mut attempt: Attempt,
    sources: Sources,
    helper: tls::FakeTls,
    before: Option<pins::Pins>,
    captures: (Option<actors::Captured>, Option<actors::Captured>),
    execution: std::thread::Result<bool>,
    overall: Instant,
) {
    let (version, negative) = captures;
    let address = helper.address;
    let ipv6 = attempt.proxy.as_ref().unwrap().quiet_ipv6();
    let outcome = attempt.proxy.take().unwrap().stop_and_join();
    report_proxy(&outcome);
    let helper_result = caught(|| helper.finish(overall));
    for (label, capture) in [
        ("client-reason-version", &version),
        ("client-reason-negative", &negative),
    ] {
        if let Some(capture) = capture {
            evidence::fake_capture(label, capture);
        }
    }
    let denied = caught(|| attempt.require_denied_quiet()).is_ok();
    let controls = caught(|| attempt.verify_controls()).is_ok();
    let source_valid = sources.verify();
    let pins_valid = before
        .as_ref()
        .is_some_and(|p| caught(|| pins::unchanged(p, overall)).is_ok());
    if before.is_none() {
        let _ = caught(|| pins::observe(overall));
    }
    let scan = scan_secret(&attempt.root, FAKE, overall);
    eprintln!(
        "phase=client-reason-scan complete={} matches={:?}",
        scan.is_ok(),
        scan.as_ref().ok().map(|s| s.count())
    );
    let metadata = metadata_rows(&sources);
    let observation = negative
        .as_ref()
        .and_then(|c| c.original_identity.as_ref())
        .and_then(|id| {
            metadata
                .as_ref()
                .ok()
                .and_then(|b| client_reason_metadata::parse(b, &attempt.nonce, id).ok())
        });
    eprintln!(
        "phase=client-reason metadata={observation:?} sources={source_valid} pins={pins_valid} controls={controls} denied={denied} ipv6={ipv6} roots=retained"
    );
    assert!(
        source_valid
            && pins_valid
            && controls
            && denied
            && ipv6
            && scan.is_ok_and(|s| s.is_empty()),
        "observer controls unavailable"
    );
    let proxy_ready = execution.expect("observer execution unavailable; roots retained");
    let capture = negative.expect("negative capture unavailable");
    let (helper_capture, verdict) = helper_result.expect("helper settlement unavailable");
    assert!(proxy_ready, "original negative proxy deadline expired");
    assert!(
        actors::is_refusal(&capture) && certificate_error(&capture.stdout, &capture.stderr),
        "strict CLI certificate refusal required"
    );
    actors::require_clean(&helper_capture, "client-reason-helper");
    assert!(helper_capture.stderr.is_empty(), "helper stderr forbidden");
    let verdict = verdict.expect("strict helper metadata");
    assert!(verdict.port == address.port(), "helper readiness identity");
    require_proxy(
        &outcome.expect("original proxy joined"),
        &verdict,
        address,
        Mode::ClientReason,
    );
    assert!(
        observation.is_some_and(|o| o.complete && o.healthy),
        "observer completion unavailable"
    );
    attempt.cleanup();
}
fn invoke(attempt: &Attempt, sources: &Sources, version: bool, until: Instant) -> actors::Captured {
    let command = attempt.command();
    let mut env = sources.environment(attempt, version);
    if !version {
        env.push((
            "CLAUDE_CODE_OAUTH_TOKEN".into(),
            "harflow-invalid-negative-token".into(),
        ));
    }
    let args = if version {
        vec![OsString::from("--version")]
    } else {
        command.args.iter().map(OsString::from).collect()
    };
    actors::run(
        &attempt.root,
        &attempt.profile,
        Path::new(&command.executable),
        &args,
        env,
        until,
        if version { 8192 } else { 1_048_576 },
    )
}
fn version_valid(capture: &actors::Captured, sources: &Sources, attempt: &Attempt) -> bool {
    let metadata = sources.version.read_bounded();
    let observed = capture.original_identity.as_ref().and_then(|id| {
        metadata
            .as_ref()
            .ok()
            .and_then(|b| client_reason_metadata::parse(b, &attempt.nonce, id).ok())
    });
    capture.complete
        && !capture.status.deadline_expired
        && capture.exit == ProcessExit::Code(0)
        && capture.stdout == b"2.1.318 (Claude Code)\n"
        && capture.stderr.is_empty()
        && observed.is_some_and(|o| {
            o.complete
                && o.healthy
                && o.reported_calls == Some(0)
                && o.calls.is_empty()
                && o.records.is_empty()
                && !o.overflow
        })
}

fn metadata_rows(sources: &Sources) -> std::io::Result<Vec<u8>> {
    let version = sources.version.read_bounded();
    let negative = sources.negative.read_bounded();
    eprintln!(
        "phase=client-reason-files version-valid={} version-length={} negative-valid={} negative-length={}",
        version.is_ok(),
        version.as_ref().map_or(0, Vec::len),
        negative.is_ok(),
        negative.as_ref().map_or(0, Vec::len)
    );
    negative
}
