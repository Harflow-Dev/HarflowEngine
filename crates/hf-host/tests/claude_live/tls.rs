use super::{PYTHON, TLS_HELPER, actors, environment};
use hf_platform::{
    process::{MacIdentity, RetainedOwnedChild},
    qualification::FixtureRoot,
};
use std::{
    env, fs,
    time::{Duration, Instant},
};
pub(super) fn start_fake_tls(root: &FixtureRoot, until: Instant) -> FakeTls {
    let directory = root.outside().join("tlsnegative");
    fs::create_dir(&directory).unwrap();
    let spec = hf_platform::process::SpawnSpec {
        executable: fs::canonicalize(PYTHON).expect("physical trusted TLS helper interpreter"),
        cwd: env::current_dir().unwrap(),
        args: vec![
            "-I".into(),
            "-S".into(),
            TLS_HELPER.into(),
            directory.into_os_string(),
            "25".into(),
        ],
        environment: environment(&[]),
        capture_limit: 4096,
    };
    let child = RetainedOwnedChild::spawn(
        spec,
        &MacIdentity,
        until.min(Instant::now() + Duration::from_secs(3)),
    )
    .unwrap();
    let mut child = actors::Original::new(child);
    while Instant::now() < until {
        child.0.pump(until, 1024).unwrap();
        if let Some(line) = child.0.capture().stdout.bytes.split(|b| *b == b'\n').next()
            && let Ok(s) = std::str::from_utf8(line)
            && let Some(port) = s
                .split("\"port\":")
                .nth(1)
                .and_then(|x| x.split('}').next())
                .and_then(|x| x.trim().parse::<u16>().ok())
        {
            return FakeTls {
                child: Some(child),
                address: ([127, 0, 0, 1], port).into(),
            };
        }
    }
    panic!("fake TLS helper readiness unavailable");
}
pub(super) struct FakeTls {
    child: Option<actors::Original>,
    pub(super) address: std::net::SocketAddr,
}
pub(super) struct TlsVerdict {
    pub(super) accepted: bool,
    pub(super) certificate_rejected: bool,
    pub(super) application_data: bool,
}
impl FakeTls {
    pub(super) fn finish(mut self, until: Instant) -> TlsVerdict {
        let result = self.child.take().unwrap().collect(until);
        actors::require_clean(&result, "fake-TLS-helper");
        assert!(result.stderr.is_empty(), "fake TLS helper stderr forbidden");
        let output = String::from_utf8(result.stdout).expect("static helper verdict");
        TlsVerdict {
            accepted: output.contains("\"accepted\": true"),
            certificate_rejected: output.contains("\"certificate_rejected\": true"),
            application_data: output.contains("\"application_data\": true"),
        }
    }
}
