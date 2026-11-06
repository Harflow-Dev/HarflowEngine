use hf_backends::{claude_live_probe::live_command, claude_measurement::ClaudeFixtureRequest};
use hf_platform::qualification::{
    FixtureRoot,
    live::{TwoTunnelProxy, scan_secret},
};
use std::{
    env,
    ffi::OsString,
    fs,
    net::TcpListener,
    path::Path,
    time::{Duration, Instant},
};
#[path = "../claude_live/actors.rs"]
mod actors;
mod attempt;
#[path = "../claude_live/canary.rs"]
mod canary;
mod codec;
mod evidence;
#[path = "../claude_live/identity.rs"]
mod identity;
#[allow(dead_code)] // Each integration target selects only its own diagnostic entry.
mod milestone_codec;
#[path = "../claude_live/pins.rs"]
mod pins;
mod support;
mod tls;
use attempt::Attempt;
use support::*;
const PYTHON: &str = "/Library/Developer/CommandLineTools/usr/bin/python3";
const HELPER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/claude_measurement/preflight.py"
);
const TLS_HELPER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/claude_two_tunnel/fake_tls.py"
);

#[allow(dead_code)]
pub(super) fn run() {
    run_mode(Mode::TwoTunnel);
}
#[allow(dead_code)]
pub(super) fn run_replacement_wave() {
    run_mode(Mode::ReplacementWave);
}
#[allow(dead_code)]
pub(super) fn run_handshake_diagnostic() {
    run_mode(Mode::HandshakeDiagnostic);
}
#[allow(dead_code)]
#[derive(Clone, Copy)]
enum Mode {
    TwoTunnel,
    ReplacementWave,
    HandshakeDiagnostic,
}
impl Mode {
    fn maximum(self) -> usize {
        match self {
            Self::TwoTunnel => 2,
            Self::ReplacementWave | Self::HandshakeDiagnostic => 4,
        }
    }
    fn gate(self) -> &'static str {
        match self {
            Self::TwoTunnel => "HARFLOW_RUN_TWO_TUNNEL_TLS_NEGATIVE",
            Self::ReplacementWave => "HARFLOW_RUN_REPLACEMENT_WAVE_TLS_NEGATIVE",
            Self::HandshakeDiagnostic => "HARFLOW_RUN_HANDSHAKE_EOF_DIAGNOSTIC",
        }
    }
}
fn run_mode(mode: Mode) {
    assert_eq!(
        env::var(mode.gate()).as_deref(),
        Ok("1"),
        "explicit fake-only gate required"
    );
    eprintln!("phase=fake-only-mode total={} active=2", mode.maximum());
    let overall = Instant::now() + Duration::from_secs(130);
    let selected = identity::selection();
    let root = FixtureRoot::create().expect("fresh fake-only fixture root");
    prepare_dirs(&root);
    let helper = tls::start(
        &root,
        overall.min(Instant::now() + Duration::from_secs(30)),
        mode,
    );
    let helper_address = helper.address;
    let mut attempt = Attempt::from_root(root, helper_address, overall, selected, mode);
    let captured =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vendor(&attempt, overall)));
    let (captured, negative_until) = match captured {
        Ok(value) => value,
        Err(failure) => {
            eprintln!("phase=two-tunnel-controller reason=phase-unwind roots=retained");
            let outcome = attempt.proxy.take().unwrap().stop_and_join();
            report_proxy(&outcome);
            let _ = helper.finish(overall);
            let scan = scan_secret(&attempt.root, b"harflow-invalid-negative-token", overall);
            eprintln!(
                "phase=two-tunnel-scan complete={} matches={:?}",
                scan.is_ok(),
                scan.as_ref().ok().map(|v| v.count())
            );
            std::panic::resume_unwind(failure);
        }
    };
    evidence::fake_capture("two-tunnel-vendor", &captured);
    let certificate = certificate_error(&captured.stdout, &captured.stderr);
    eprintln!("phase=two-tunnel-vendor static-certificate-error={certificate}");
    let proxy_observed = await_proxy(attempt.proxy.as_ref().unwrap(), negative_until);
    eprintln!("phase=two-tunnel-proxy observed-within-negative-deadline={proxy_observed}");
    let outcome = attempt.proxy.take().unwrap().stop_and_join();
    report_proxy(&outcome);
    let (helper_capture, verdict) = helper.finish(overall);
    let scan = scan_secret(&attempt.root, b"harflow-invalid-negative-token", overall);
    eprintln!(
        "phase=two-tunnel-scan complete={} matches={:?}",
        scan.is_ok(),
        scan.as_ref().ok().map(|v| v.count())
    );
    attempt.require_denied_quiet();
    attempt.verify_controls();
    let outcome = outcome.expect("original proxy joined");
    assert!(
        proxy_observed,
        "proxy observation expired within original negative deadline"
    );
    assert!(
        actors::is_refusal(&captured) && certificate,
        "strict CLI certificate refusal required"
    );
    actors::require_clean(&helper_capture, "two-tunnel-helper");
    assert!(helper_capture.stderr.is_empty(), "helper stderr forbidden");
    let verdict = verdict.expect("strict helper metadata");
    assert_eq!(
        verdict.port,
        helper_address.port(),
        "helper readiness identity"
    );
    require_proxy(&outcome, &verdict, helper_address, mode);
    assert!(
        scan.expect("bounded fake-token scan").is_empty(),
        "fake token persisted"
    );
    attempt.cleanup();
}

fn vendor(attempt: &Attempt, overall: Instant) -> (actors::Captured, Instant) {
    attempt.preflight_and_version(overall);
    let command = attempt.command();
    let mut environment = environment(&command.env);
    environment.push((
        "CLAUDE_CODE_OAUTH_TOKEN".into(),
        "harflow-invalid-negative-token".into(),
    ));
    let args = command.args.iter().map(OsString::from).collect::<Vec<_>>();
    let negative_until = overall.min(Instant::now() + Duration::from_secs(15));
    let captured = actors::run(
        &attempt.root,
        &attempt.profile,
        Path::new(&command.executable),
        &args,
        environment,
        negative_until,
        1_048_576,
    );
    (captured, negative_until)
}

fn await_proxy(proxy: &TwoTunnelProxy, until: Instant) -> bool {
    while Instant::now() < until {
        if proxy.observation_ready() {
            return Instant::now() < until;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    false
}

fn require_proxy(
    outcome: &hf_platform::qualification::live::TwoTunnelOutcome,
    verdict: &codec::Verdict,
    helper_address: std::net::SocketAddr,
    mode: Mode,
) {
    assert!(
        outcome.preflights == 1
            && outcome.accepted > 0
            && outcome.accepted <= mode.maximum()
            && outcome.connects == outcome.accepted
            && outcome.slots.len() == outcome.accepted
            && outcome.ipv6_connections == 0
            && !outcome.failed
            && outcome.owned_sockets_closed
            && outcome.worker_joined
            && budget_witness(outcome),
        "all admitted proxy sessions must settle"
    );
    let upstreams: Vec<_> = outcome
        .slots
        .iter()
        .filter_map(|slot| slot.upstream_local.map(|a| (slot.ordinal, a)))
        .collect();
    assert!(
        outcome.slots.iter().all(|s| s.client_closed
            && s.upstream_closed
            && s.upstream_peer == Some(helper_address)
            && direction_settled(&s.client_to_upstream)
            && direction_settled(&s.upstream_to_client))
            && upstreams.len() == outcome.accepted
            && match mode {
                Mode::TwoTunnel => codec::bound_rejection(verdict, &upstreams),
                Mode::ReplacementWave | Mode::HandshakeDiagnostic =>
                    codec::bound_wave_rejection(verdict, &upstreams),
            },
        "bound explicit certificate rejection required for every admitted session"
    );
}

fn report_proxy(outcome: &std::io::Result<hf_platform::qualification::live::TwoTunnelOutcome>) {
    match outcome {
        Ok(value) => eprintln!("phase=two-tunnel-proxy outcome={value:?}"),
        Err(error) => eprintln!(
            "phase=two-tunnel-proxy reason=join-error kind={:?}",
            error.kind()
        ),
    }
}

fn direction_settled(value: &hf_platform::qualification::live::TwoTunnelDirection) -> bool {
    value.eof
        && value.write_half_closed
        && value.buffered_bytes == 0
        && value.bytes_read == value.bytes_forwarded
}

fn budget_witness(outcome: &hf_platform::qualification::live::TwoTunnelOutcome) -> bool {
    (1..=2).contains(&outcome.peak_active_slots)
        && outcome.retired_slots_closed <= outcome.accepted
        && outcome.peak_client_to_upstream_pending <= 16384
        && outcome.peak_upstream_to_client_pending <= 16384
        && [true, false].into_iter().all(|outgoing| {
            outcome
                .slots
                .iter()
                .try_fold(0usize, |total, slot| {
                    total.checked_add(if outgoing {
                        slot.client_to_upstream.bytes_read
                    } else {
                        slot.upstream_to_client.bytes_read
                    })
                })
                .is_some_and(|total| total <= 262144)
        })
}
