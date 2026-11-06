#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_backends::{claude_live_probe::live_command, claude_measurement::ClaudeFixtureRequest};
use hf_interfaces::{
    backend_measurement::{ClaudeEvent, decode_claude_event},
    claude_live_credential::parse_oauth_access,
};
use hf_platform::qualification::{
    FixtureRoot,
    live::{OriginalActorsSettled, Proxy, ProxyMode, scan_secret},
};
use std::{
    env,
    ffi::OsString,
    fs,
    net::TcpListener,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[path = "actors.rs"]
mod actors;
#[path = "canary.rs"]
mod canary;
#[path = "evidence.rs"]
mod evidence;
#[path = "identity.rs"]
mod identity;
#[path = "pins.rs"]
mod pins;
#[path = "support.rs"]
mod support;
use support::*;
#[path = "tls.rs"]
mod tls;
use tls::start_fake_tls;

const PYTHON: &str = "/Library/Developer/CommandLineTools/usr/bin/python3";
const HELPER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/claude_measurement/preflight.py"
);
const TLS_HELPER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/claude_live/fake_tls.py");

pub(super) fn run() {
    assert_eq!(
        env::var("HARFLOW_RUN_LIVE_CLAUDE").as_deref(),
        Ok("1"),
        "explicit primary live gate required"
    );
    let overall = Instant::now() + Duration::from_secs(130);
    negative(overall);
    real(overall);
}

pub(super) fn run_tls_negative_diagnostic() {
    assert_eq!(
        env::var("HARFLOW_RUN_TLS_NEGATIVE_DIAGNOSTIC").as_deref(),
        Ok("1"),
        "explicit fake-only diagnostic gate required"
    );
    negative(Instant::now() + Duration::from_secs(130));
}

fn negative(overall: Instant) {
    let selected = identity::selection();
    let negative_root = FixtureRoot::create().expect("fresh negative fixture root");
    prepare_dirs(&negative_root);
    let fake = start_fake_tls(
        &negative_root,
        overall.min(Instant::now() + Duration::from_secs(30)),
    );
    let mut negative = Attempt::from_root(
        negative_root,
        ProxyMode::UntrustedTls(fake.address),
        overall,
        selected,
    );
    negative.preflight_and_version(overall);
    let command = negative.command();
    let mut env = environment(&command.env);
    env.push((
        "CLAUDE_CODE_OAUTH_TOKEN".into(),
        "harflow-invalid-negative-token".into(),
    ));
    let args = command.args.iter().map(OsString::from).collect::<Vec<_>>();
    let refused = actors::run(
        &negative.root,
        &negative.profile,
        Path::new(&command.executable),
        &args,
        env,
        overall.min(Instant::now() + Duration::from_secs(15)),
        1_048_576,
    );
    evidence::fake_capture("fake-only-vendor", &refused);
    evidence::certificate_error(certificate_error(&refused.stdout, &refused.stderr));
    let outcome = negative.proxy.take().unwrap().stop_and_join().unwrap();
    evidence::fake_proxy(&outcome);
    let verdict = fake.finish(overall);
    assert!(
        actors::is_refusal(&refused),
        "untrusted TLS negative did not refuse"
    );
    assert!(
        certificate_error(&refused.stdout, &refused.stderr),
        "CLI did not report a static certificate-verification failure"
    );
    assert!(proxy_witness(&outcome, 1));
    assert!(
        verdict.accepted && verdict.certificate_rejected && !verdict.application_data,
        "fake TLS negative lacked independent rejection/no-data witnesses"
    );
    assert!(
        scan_secret(&negative.root, b"harflow-invalid-negative-token", overall)
            .unwrap()
            .is_empty()
    );
    negative.cleanup();
}

fn real(overall: Instant) {
    let real = Attempt::new(ProxyMode::Live, overall);
    real.preflight_and_version(overall);
    assert!(
        Instant::now() < overall,
        "shared deadline elapsed before credential read"
    );
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let minimum = now_ms.checked_add(130_000).expect("credential clock bound");
    let raw = hf_platform::qualification::live::read_credential_record(
        overall.min(Instant::now() + Duration::from_secs(8)),
    )
    .unwrap();
    let access = parse_oauth_access(&raw, now_ms, minimum).unwrap();
    drop(raw);
    let final_pins = pins::observe(overall);
    let command = real.command();
    let mut env = environment(&command.env);
    let (key, value) = access.environment_pair();
    env.push((key.into(), value.into()));
    let args = command.args.iter().map(OsString::from).collect::<Vec<_>>();
    let result = actors::run(
        &real.root,
        &real.profile,
        Path::new(&command.executable),
        &args,
        env,
        overall.min(Instant::now() + Duration::from_secs(90)),
        65_536,
    );
    finish_real(real, result, value, &final_pins, overall);
}

fn finish_real(
    mut real: Attempt,
    result: actors::Captured,
    value: &str,
    final_pins: &pins::Pins,
    overall: Instant,
) {
    let output_secret = [result.stdout.as_slice(), result.stderr.as_slice()]
        .iter()
        .any(|bytes| {
            bytes
                .windows(value.len())
                .any(|window| window == value.as_bytes())
        });
    let outcome = real
        .proxy
        .take()
        .unwrap()
        .stop_and_join()
        .expect("original proxy joined");
    let settled = OriginalActorsSettled::attest_original_waits_eof_and_joins();
    let matches =
        scan_secret(&real.root, value.as_bytes(), overall).expect("bounded owned-root scan");
    if !matches.is_empty() {
        let expected = matches.count();
        let removed = hf_platform::qualification::live::remove_matched_files(
            &real.root,
            matches,
            &settled,
            Instant::now() + Duration::from_secs(5),
        );
        match removed {
            Ok(count) => panic!("credential literal persisted: matched={expected} removed={count}"),
            Err(_) => panic!("credential literal persisted: matched={expected} removal failed"),
        }
    }
    assert!(
        !output_secret,
        "credential literal appeared in volatile captures"
    );
    actors::require_clean(&result, "live-vendor");
    assert!(
        result.stderr.is_empty(),
        "successful live CLI emitted stderr"
    );
    require_protocol(&result.stdout, &real.nonce);
    assert!(proxy_witness(&outcome, 1));
    pins::unchanged(final_pins, overall);
    evidence::real(&result, &outcome);
    real.cleanup();
}

struct Attempt {
    root: FixtureRoot,
    nonce: String,
    profile: String,
    proxy: Option<Proxy>,
    denied: TcpListener,
    selected: identity::VendorSelection,
    outside: canary::Canary,
    owner: canary::Canary,
}

impl Attempt {
    fn new(mode: ProxyMode, until: Instant) -> Self {
        let root = FixtureRoot::create().expect("fresh private fixture root");
        prepare_dirs(&root);
        Self::from_root(root, mode, until, identity::selection())
    }
    fn from_root(
        root: FixtureRoot,
        mode: ProxyMode,
        until: Instant,
        selected: identity::VendorSelection,
    ) -> Self {
        let nonce = identity::nonce_from_root(root.root());
        let proxy = Proxy::start(mode, &nonce, until).expect("exclusive loopback proxy pair");
        let profile = hf_platform::qualification::live::live_profile(&root, proxy.port())
            .expect("recorded profile plus credential denial");
        let denied = TcpListener::bind(("127.0.0.1", 0)).expect("denied-port control listener");
        denied.set_nonblocking(true).unwrap();
        let outside = canary::Canary::create(
            root.root()
                .parent()
                .unwrap()
                .join(format!("{nonce}-live-read-canary")),
            b"O",
        );
        let owner =
            canary::Canary::create(root.protected().join("owner-control-sentinel"), b"owner");
        let attempt = Self {
            outside,
            owner,
            root,
            nonce,
            profile,
            proxy: Some(proxy),
            denied,
            selected,
        };
        evidence::instance(&attempt);
        attempt
    }
    fn preflight_and_version(&self, until: Instant) {
        assert_eq!(
            self.selected.expected_hash,
            "677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d"
        );
        self.preflight(until);
        let version_until = until.min(Instant::now() + Duration::from_secs(15));
        let before = pins::observe(version_until);
        let command = self.command();
        let version = actors::run(
            &self.root,
            &self.profile,
            Path::new(&command.executable),
            &["--version".into()],
            environment(&command.env),
            version_until,
            8192,
        );
        actors::require_clean(&version, "pinned-version");
        assert_eq!(version.stdout, b"2.1.318 (Claude Code)\n");
        assert!(version.stderr.is_empty(), "version stderr forbidden");
        pins::unchanged(&before, version_until);
        evidence::version(self);
    }
    fn preflight(&self, until: Instant) {
        let until = until.min(Instant::now() + Duration::from_secs(5));
        self.spawn_control(until);
        self.verify_controls();
        let outside = self.outside.path();
        let inside = self.root.runtime().join("inside-canary");
        let owner = self.owner.path();
        fs::write(&inside, b"I").unwrap();
        let port = self.proxy.as_ref().unwrap().port().to_string();
        let denied = self.denied.local_addr().unwrap().port().to_string();
        let args = [
            "-I",
            "-S",
            HELPER,
            "--owner-control-denial",
            owner.to_str().unwrap(),
            outside.to_str().unwrap(),
            inside.to_str().unwrap(),
            &port,
            &denied,
            &self.nonce,
        ]
        .map(OsString::from);
        let env = environment(&[(
            "HOME".into(),
            self.root.runtime().to_string_lossy().into_owned(),
        )]);
        let result = actors::run(
            &self.root,
            &self.profile,
            Path::new(PYTHON),
            &args,
            env,
            until,
            4096,
        );
        actors::require_clean(&result, "same-profile-preflight");
        self.verify_controls();
        self.require_denied_quiet();
        assert!(
            String::from_utf8_lossy(&result.stdout).contains(
                "fork=1 posix_spawn=1 read=1 inside=1 allowed=1 denied_port=1 owner_write_denied=1"
            ),
            "same-profile preflight failed"
        );
    }
    fn command(&self) -> hf_backends::claude_measurement::ClaudeFixtureCommand {
        live_command(
            &request(
                &self.root,
                &self.nonce,
                self.proxy.as_ref().unwrap().port(),
                &self.selected.logical,
            ),
            self.proxy.as_ref().unwrap().port(),
        )
        .unwrap()
    }
    fn require_denied_quiet(&self) {
        assert!(
            matches!(self.denied.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "denied listener received connection"
        );
    }
    fn verify_controls(&self) {
        self.outside.verify();
        self.owner.verify();
    }
    fn spawn_control(&self, until: Instant) {
        let profile = self
            .root
            .profile(
                hf_platform::qualification::ProfilePolicy::ControlledHelper,
                &hf_platform::qualification::qualification_read_paths(),
                Some(self.proxy.as_ref().unwrap().port()),
            )
            .unwrap();
        let args = ["-I", "-S", HELPER, "--spawn-control"].map(OsString::from);
        let result = actors::run(
            &self.root,
            &profile,
            Path::new(PYTHON),
            &args,
            environment(&[]),
            until,
            4096,
        );
        actors::require_clean(&result, "controlled-spawn-positive");
        assert_eq!(result.stdout, b"spawn-control=allowed\n");
        assert!(result.stderr.is_empty());
    }
    fn cleanup(self) {
        self.require_denied_quiet();
        self.verify_controls();
        let root = self.root.root().to_path_buf();
        self.root
            .validate_topology()
            .expect("settled root topology");
        self.outside.remove();
        fs::remove_dir_all(&root).expect("remove verified owned fixture root");
        eprintln!(
            "phase=cleanup root={} nonce={} owned-root=removed original-read-canary=removed actors=settled",
            root.display(),
            self.nonce
        );
    }
}
