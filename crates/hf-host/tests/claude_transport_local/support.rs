use super::*;

const PYTHON: &str = "/Library/Developer/CommandLineTools/usr/bin/python3";
const HELPER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/claude_two_tunnel/fake_tls.py"
);
const DRIVER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/claude_transport_local/scenario.py"
);
const NONCE: &str = "a1b2c3d4e5f60718";
const PREFLIGHT: &[u8] =
    b"GET /a1b2c3d4e5f60718/preflight HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";

struct Helper {
    child: actors::Original,
    address: SocketAddr,
}

fn env_clear() -> Vec<(OsString, OsString)> {
    [
        ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin"),
        ("LANG", "C"),
        ("LC_ALL", "C"),
        ("PYTHONNOUSERSITE", "1"),
        ("PYTHONDONTWRITEBYTECODE", "1"),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
    .collect()
}

fn start_helper(root: &FixtureRoot, label: &str) -> Helper {
    let directory = root.outside().join(label);
    fs::create_dir(&directory).expect("new isolated certificate directory");
    let spec = SpawnSpec {
        executable: fs::canonicalize(PYTHON).expect("pinned system Python path"),
        cwd: std::env::current_dir().expect("test working directory"),
        args: vec![
            "-I".into(),
            "-S".into(),
            HELPER.into(),
            directory.into_os_string(),
            "25".into(),
        ],
        environment: {
            let mut values = env_clear();
            values.push(("HOME".into(), root.workspace().into_os_string()));
            values
        },
        capture_limit: 4096,
    };
    let child =
        RetainedOwnedChild::spawn(spec, &MacIdentity, Instant::now() + Duration::from_secs(3))
            .expect("fixed local TLS helper child");
    let mut child = actors::Original::new(child);
    let until = Instant::now() + Duration::from_secs(8);
    let mut port = None;
    while Instant::now() < until {
        child
            .0
            .pump(until, 1024)
            .expect("bounded helper readiness pump");
        if let Ok(value) = codec::readiness(&child.0.capture().stdout.bytes) {
            port = Some(value);
            break;
        }
        assert_eq!(child.0.try_wait().unwrap(), ProcessExit::Unobserved);
    }
    let port = port.unwrap_or_else(|| {
        let result = child.collect(Instant::now() + Duration::from_secs(27));
        panic!(
            "helper readiness absent; settled={} exit={:?}",
            result.complete, result.exit
        )
    });
    Helper {
        child,
        address: ([127, 0, 0, 1], port).into(),
    }
}

fn run_child(args: Vec<OsString>, until: Instant) -> actors::Captured {
    let spec = SpawnSpec {
        executable: fs::canonicalize(PYTHON).expect("pinned system Python path"),
        cwd: std::env::current_dir().expect("test working directory"),
        args,
        environment: env_clear(),
        capture_limit: 4096,
    };
    let child = RetainedOwnedChild::spawn(
        spec,
        &MacIdentity,
        until.min(Instant::now() + Duration::from_secs(3)),
    )
    .expect("bounded local scenario child");
    actors::Original::new(child).collect(until)
}

fn finish_helper(mut helper: Helper) -> (SocketAddr, codec::Verdict) {
    let address = helper.address;
    let result = helper
        .child
        .collect(Instant::now() + Duration::from_secs(27));
    actors::require_clean(&result, "local TLS helper");
    assert!(result.stderr.is_empty(), "helper stderr must be empty");
    let verdict = codec::parse(&result.stdout).expect("strict helper metadata");
    assert_eq!(verdict.sessions.len(), 2);
    assert!(verdict.sessions.iter().all(|session| session.closed));
    assert_eq!(verdict.port, address.port());
    (address, verdict)
}

fn require_direct_pair(verdict: &codec::Verdict) {
    assert_eq!(verdict.sessions.len(), 2);
    let mut sessions = verdict.sessions.iter().collect::<Vec<_>>();
    sessions.sort_by_key(|session| session.ordinal);
    assert_eq!(sessions[0].ordinal, 1);
    assert_eq!(sessions[1].ordinal, 2);
    assert!(sessions[0].application_data);
    assert_eq!(sessions[0].phase, codec::Phase::ApplicationRead);
    assert!(sessions[1].certificate_rejected);
    assert_eq!(sessions[1].phase, codec::Phase::Handshake);
}

fn require_trusted_pair(verdict: &codec::Verdict) {
    assert_eq!(verdict.sessions.len(), 2);
    let mut sessions = verdict.sessions.iter().collect::<Vec<_>>();
    sessions.sort_by_key(|session| session.ordinal);
    for (index, session) in sessions.iter().enumerate() {
        assert_eq!(session.ordinal, index + 1);
        assert!(session.application_data);
        assert_eq!(session.phase, codec::Phase::ApplicationRead);
    }
}

fn require_proxy_binding(
    outcome: &hf_platform::qualification::live::TwoTunnelOutcome,
    address: SocketAddr,
    verdict: &codec::Verdict,
) {
    assert_eq!(outcome.slots.len(), 2);
    let mut slots = outcome.slots.iter().collect::<Vec<_>>();
    slots.sort_by_key(|slot| slot.ordinal);
    let mut sessions = verdict.sessions.iter().collect::<Vec<_>>();
    sessions.sort_by_key(|session| session.ordinal);
    for (index, (slot, session)) in slots.iter().zip(sessions).enumerate() {
        assert_eq!(slot.ordinal, index + 1);
        assert_eq!(session.ordinal, index + 1);
        assert_eq!(slot.upstream_peer, Some(address));
        assert_eq!(slot.upstream_local.unwrap().port(), session.peer_port);
        for direction in [&slot.client_to_upstream, &slot.upstream_to_client] {
            assert!(direction.eof && direction.write_half_closed);
            assert_eq!(direction.buffered_bytes, 0);
            assert_eq!(direction.bytes_read, direction.bytes_forwarded);
            assert!(direction.bytes_forwarded > 0);
        }
        assert!(slot.client_closed && slot.upstream_closed);
    }
}

fn start_proxy(address: SocketAddr) -> TwoTunnelProxy {
    TwoTunnelProxy::start(address, NONCE, Instant::now() + Duration::from_secs(25))
        .expect("fixed local two-tunnel proxy")
}

fn preflight(proxy: &TwoTunnelProxy) {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(PREFLIGHT).unwrap();
    let mut response = Vec::new();
    use std::io::Read;
    stream.take(4096).read_to_end(&mut response).unwrap();
    assert!(response.windows(12).any(|part| part == b"preflight-ok"));
}

pub(super) fn direct_pair() {
    let root = FixtureRoot::create().expect("isolated fixture root");
    let helper = start_helper(&root, "direct");
    let ca = root.outside().join("direct/certificate.pem");
    let result = run_child(
        vec![
            "-I".into(),
            "-S".into(),
            DRIVER.into(),
            "direct".into(),
            helper.address.port().to_string().into(),
            ca.into_os_string(),
        ],
        Instant::now() + Duration::from_secs(8),
    );
    actors::require_clean(&result, "direct TLS controls");
    assert!(result.stderr.is_empty());
    let lines = std::str::from_utf8(&result.stdout)
        .unwrap()
        .lines()
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    let ports = [0, 1].map(|index| {
        lines[index]
            .strip_prefix(if index == 0 {
                "TRUSTED\tapplication_data\tTLSv1.2\tALPN-none\tSAN-self-issued\t"
            } else {
                "DEFAULT\tcertificate-verification-failed\t"
            })
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|value| *value != 0)
            .unwrap()
    });
    let (_, verdict) = finish_helper(helper);
    require_direct_pair(&verdict);
    let mut sessions = verdict.sessions.iter().collect::<Vec<_>>();
    sessions.sort_by_key(|session| session.ordinal);
    assert_eq!(sessions[0].peer_port, ports[0]);
    assert_eq!(sessions[1].peer_port, ports[1]);
}

pub(super) fn trusted_proxy_pair() {
    let root = FixtureRoot::create().expect("isolated fixture root");
    let helper = start_helper(&root, "proxy");
    let ca = root.outside().join("proxy/certificate.pem");
    let proxy = start_proxy(helper.address);
    preflight(&proxy);
    let result = run_child(
        vec![
            "-I".into(),
            "-S".into(),
            DRIVER.into(),
            "proxy".into(),
            proxy.port().to_string().into(),
            ca.into_os_string(),
        ],
        Instant::now() + Duration::from_secs(8),
    );
    actors::require_clean(&result, "proxied trusted TLS controls");
    assert!(result.stderr.is_empty());
    let lines = std::str::from_utf8(&result.stdout)
        .unwrap()
        .lines()
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|line| {
        line.starts_with("TRUSTED\tapplication_data\tTLSv1.2\tALPN-none\tSAN-self-issued\t")
    }));
    let outcome = joined_proxy(proxy);
    assert!(!outcome.failed);
    assert_eq!(outcome.preflights, 1);
    assert_eq!(outcome.accepted, 2);
    assert_eq!(outcome.connects, 2);
    assert!(outcome.owned_sockets_closed && outcome.worker_joined);
    let (address, verdict) = finish_helper(helper);
    require_trusted_pair(&verdict);
    require_proxy_binding(&outcome, address, &verdict);
}

#[path = "curl.rs"]
mod curl;
pub(super) fn curl_negative() {
    curl::run();
}

fn joined_proxy(proxy: TwoTunnelProxy) -> hf_platform::qualification::live::TwoTunnelOutcome {
    let until = Instant::now() + Duration::from_secs(2);
    while !proxy.observation_ready() && Instant::now() < until {
        std::thread::yield_now();
    }
    let observed = proxy.observation_ready();
    let outcome = proxy.stop_and_join().expect("original local proxy joins");
    assert!(
        observed,
        "local proxy observation did not settle before join"
    );
    outcome
}
