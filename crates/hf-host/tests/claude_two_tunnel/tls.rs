use super::*;
use hf_platform::process::{MacIdentity, RetainedOwnedChild, SpawnSpec};
pub(super) struct FakeTls {
    child: actors::Original,
    mode: Mode,
    pub(super) address: std::net::SocketAddr,
}
pub(super) fn start(root: &FixtureRoot, until: Instant, mode: Mode) -> FakeTls {
    let directory = root.outside().join("two-tunnel-tls");
    fs::create_dir(&directory).expect("fresh TLS directory");
    let spec = SpawnSpec {
        executable: fs::canonicalize(PYTHON).expect("physical trusted interpreter"),
        cwd: env::current_dir().expect("controller cwd"),
        args: vec![
            "-I".into(),
            "-S".into(),
            match mode {
                Mode::TwoTunnel => TLS_HELPER,
                Mode::ReplacementWave => concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/claude_two_tunnel/wave_tls.py"
                ),
                Mode::HandshakeDiagnostic | Mode::ClientReason => concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/claude_two_tunnel/eof_tls.py"
                ),
            }
            .into(),
            directory.into_os_string(),
            "25".into(),
        ],
        environment: environment(&[]),
        capture_limit: if matches!(mode, Mode::ClientReason) {
            3072
        } else {
            4096
        },
    };
    let child = RetainedOwnedChild::spawn(
        spec,
        &MacIdentity,
        until.min(Instant::now() + Duration::from_secs(3)),
    )
    .expect("original TLS helper spawn");
    let mut child = actors::Original::new(child);
    while Instant::now() < until {
        if child.0.pump(until, 1024).is_err() {
            break;
        }
        let raw = &child.0.capture().stdout.bytes;
        if raw.contains(&b'\n')
            && let Ok(port) = codec::readiness(raw)
        {
            return FakeTls {
                child,
                mode,
                address: ([127, 0, 0, 1], port).into(),
            };
        }
        if child.0.try_wait().is_err()
            || child.0.exit() != hf_domain::process::ProcessExit::Unobserved
        {
            break;
        }
    }
    let result = child.collect(until);
    evidence::fake_capture("two-tunnel-helper-readiness", &result);
    panic!("TLS helper readiness unavailable");
}
impl FakeTls {
    pub(super) fn finish(
        mut self,
        until: Instant,
    ) -> (actors::Captured, Result<codec::Verdict, &'static str>) {
        let captured = self.child.collect(until);
        evidence::fake_capture("two-tunnel-helper", &captured);
        let verdict = match self.mode {
            Mode::TwoTunnel => codec::parse(&captured.stdout),
            Mode::ReplacementWave => codec::parse_wave(&captured.stdout),
            Mode::HandshakeDiagnostic | Mode::ClientReason => {
                milestone_codec::parse(&captured.stdout).map(|value| {
                    eprintln!("phase=handshake-milestones verdict={value:?}");
                    value.verdict
                })
            }
        };
        eprintln!("phase=two-tunnel-helper verdict={verdict:?}");
        (captured, verdict)
    }
}
