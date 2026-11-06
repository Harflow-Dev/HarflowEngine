use super::*;
use hf_domain::process::ProcessExit;
use hf_platform::qualification::live::{ObserverFile, TwoTunnelFailureReason};
use std::os::unix::fs::MetadataExt;
const SOURCE: &[u8] = include_bytes!("preload_probe.cjs");
const FAKE: &[u8] = b"harflow-invalid-negative-token";

pub(super) fn run() {
    assert!(
        env::var("HARFLOW_RUN_CLIENT_PRELOAD_PROBE").as_deref() == Ok("1"),
        "explicit inert probe gate required"
    );
    let overall = Instant::now() + Duration::from_secs(130);
    let selected = identity::selection();
    assert!(
        selected.expected_hash
            == "677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d",
        "exact selected vendor pin"
    );
    let root = FixtureRoot::create().expect("fresh inert probe root");
    prepare_dirs(&root);
    let source = canary::Canary::create(root.protected().join("preload_probe.cjs"), SOURCE);
    let source_hash = source_digest(source.path());
    let stamp = fs::symlink_metadata(source.path()).unwrap();
    eprintln!(
        "phase=client-preload-source dev={} inode={} hash={source_hash}",
        stamp.dev(),
        stamp.ino()
    );
    let metadata_path = root.runtime().join("client-preload-ready");
    let metadata = ObserverFile::create(&metadata_path).expect("exclusive retained metadata file");
    let target = TcpListener::bind(("127.0.0.1", 0)).expect("owned quiet target");
    target.set_nonblocking(true).unwrap();
    let attempt = Attempt::from_root(
        root,
        target.local_addr().unwrap(),
        overall,
        selected,
        Mode::ReplacementWave,
    );
    let mut before = None;
    let observed = caught(|| {
        attempt.preflight_only(overall);
        let until = overall.min(Instant::now() + Duration::from_secs(15));
        before = Some(pins::observe(until));
        version(&attempt, &metadata, &metadata_path, source.path(), until)
    });
    settle(
        (attempt, source, source_hash, metadata, target),
        observed,
        &before,
        overall,
    );
}

fn settle(
    resources: (Attempt, canary::Canary, String, ObserverFile, TcpListener),
    observed: std::thread::Result<actors::Captured>,
    before: &Option<pins::Pins>,
    overall: Instant,
) {
    let (mut attempt, source, source_hash, metadata, target) = resources;
    // No support assertion precedes original actor custody and every bookend/control.
    let ipv6_quiet = attempt.proxy.as_ref().unwrap().quiet_ipv6();
    let outcome = attempt.proxy.take().unwrap().stop_and_join();
    let target_quiet =
        matches!(target.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock);
    let denied = caught(|| attempt.require_denied_quiet()).is_ok();
    let controls = caught(|| attempt.verify_controls()).is_ok();
    let source_valid = caught(|| {
        source.verify();
        assert!(
            source_digest(source.path()) == source_hash,
            "fixed source hash bookend"
        );
    })
    .is_ok();
    let pins_valid = match before {
        Some(before) => caught(|| pins::unchanged(before, overall)).is_ok(),
        None => {
            let _ = caught(|| pins::observe(overall));
            false
        }
    };
    let scan = scan_secret(&attempt.root, FAKE, overall);
    let row = metadata.read_bounded();
    eprintln!(
        "phase=client-preload-metadata metadata-read-valid={} length={}",
        row.is_ok(),
        row.as_ref().map_or(0, Vec::len)
    );
    if let Ok(capture) = &observed {
        evidence::fake_capture("client-preload-version", capture);
    }
    let scan_clean = scan.as_ref().is_ok_and(|matches| matches.is_empty());
    eprintln!(
        "phase=client-preload-bookends source={source_valid} pins={pins_valid} controls={controls} denied={denied} target-quiet={target_quiet} ipv6-quiet={ipv6_quiet} scan={scan_clean} roots=retained"
    );
    let proxy_clean = outcome.as_ref().is_ok_and(|value| {
        value.preflights == 1
            && value.accepted == 0
            && value.connects == 0
            && value.ipv6_connections == 0
            && value.slots.is_empty()
            && value.owned_sockets_closed
            && value.worker_joined
            && value
                .first_failure
                .as_ref()
                .is_some_and(|failure| failure.reason == TwoTunnelFailureReason::ControllerStop)
    });
    let version_clean = observed.as_ref().is_ok_and(version_proven);
    let marker = match (&observed, &row) {
        (Ok(capture), Ok(bytes)) => capture.original_identity.as_ref().is_some_and(|identity| {
            super::client_preload_metadata::parse_ready(bytes, &attempt.nonce, identity).is_ok()
        }),
        _ => false,
    };
    eprintln!(
        "phase=client-preload-support version-exact={version_clean} marker-ready={marker} proxy-quiet={proxy_clean}"
    );
    assert!(
        source_valid
            && pins_valid
            && controls
            && denied
            && target_quiet
            && ipv6_quiet
            && scan_clean
            && proxy_clean,
        "inert probe controls unavailable; roots retained"
    );
    assert!(
        version_clean && marker,
        "preload not proven usable; STOP; roots retained"
    );
    attempt.cleanup();
}

fn version(
    attempt: &Attempt,
    metadata: &ObserverFile,
    metadata_path: &Path,
    source: &Path,
    until: Instant,
) -> actors::Captured {
    let command = attempt.command();
    let source = source.to_str().expect("native source path");
    assert!(
        source
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b)),
        "fixed native preload path"
    );
    let mut env = environment(&command.env);
    env.retain(|(key, _)| key != "BUN_OPTIONS");
    for (key, value) in [
        ("BUN_OPTIONS", format!("--preload={source}")),
        (
            "HARFLOW_TLS_OBSERVER_FILE",
            metadata_path.to_str().unwrap().to_owned(),
        ),
        ("HARFLOW_TLS_OBSERVER_NONCE", attempt.nonce.clone()),
        ("HARFLOW_TLS_OBSERVER_DEV", metadata.device().to_string()),
        ("HARFLOW_TLS_OBSERVER_INODE", metadata.inode().to_string()),
    ] {
        env.push((key.into(), value.into()));
    }
    actors::run(
        &attempt.root,
        &attempt.profile,
        Path::new(&command.executable),
        &["--version".into()],
        env,
        until,
        8192,
    )
}
fn caught<T>(f: impl FnOnce() -> T) -> std::thread::Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
}
fn source_digest(path: &Path) -> String {
    let bytes = fs::read(path).expect("fixed protected source");
    let mut hash = hf_storage::Sha256Stream::with_limit(16384).unwrap();
    hash.update(&bytes).unwrap();
    hash.finish().unwrap().as_str().to_owned()
}

fn version_proven(capture: &actors::Captured) -> bool {
    capture.complete
        && !capture.status.deadline_expired
        && capture.exit == ProcessExit::Code(0)
        && capture.stdout == b"2.1.318 (Claude Code)\n"
        && capture.stderr.is_empty()
}
