#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
use hf_domain::process::ProcessExit;
use hf_platform::{
    process::{MacIdentity, RetainedOwnedChild, SpawnSpec},
    qualification::FixtureRoot,
};
use std::{
    fs,
    time::{Duration, Instant},
};
#[path = "claude_live/actors.rs"]
#[allow(dead_code)]
mod actors;
#[path = "claude_two_tunnel/codec.rs"]
#[allow(dead_code)]
mod codec;

#[test]
fn actual_wave_helper_settles_four_sessions_with_two_overlapping() {
    require_actual_rows(&collect_driver(false));
}

#[test]
fn partial_coverage_waits_original_lifetime_without_fabricated_sessions() {
    let began = Instant::now();
    let raw = collect_driver(true);
    assert!(began.elapsed() >= Duration::from_secs(25));
    let text = std::str::from_utf8(&raw).unwrap();
    let (peer, metadata) = text.split_once('\n').unwrap();
    let port = peer.strip_prefix("PEER\t").unwrap().parse::<u16>().unwrap();
    let verdict = codec::parse_wave(metadata.as_bytes()).unwrap();
    assert_eq!(verdict.sessions.len(), 1);
    assert_eq!(verdict.sessions[0].ordinal, 1);
    assert_eq!(verdict.sessions[0].peer_port, port);
    assert_eq!(verdict.sessions[0].category, codec::Category::TransportEof);
    assert_eq!(verdict.sessions[0].phase, codec::Phase::Handshake);
    assert!(!codec::bound_wave_rejection(
        &verdict,
        &[(1, ([127, 0, 0, 1], port).into())]
    ));
}

fn collect_driver(partial: bool) -> Vec<u8> {
    let root = FixtureRoot::create().unwrap();
    let directory = root.outside().join("wave-helper");
    fs::create_dir(&directory).unwrap();
    let mut spec = SpawnSpec {
        executable: fs::canonicalize("/Library/Developer/CommandLineTools/usr/bin/python3")
            .unwrap(),
        cwd: std::env::current_dir().unwrap(),
        args: vec![
            "-I".into(),
            "-S".into(),
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/claude_wave_helper/driver.py"
            )
            .into(),
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/claude_two_tunnel/wave_tls.py"
            )
            .into(),
            directory.clone().into_os_string(),
        ],
        environment: vec![
            ("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into()),
            ("HOME".into(), directory.into_os_string()),
            ("LANG".into(), "C".into()),
            ("LC_ALL".into(), "C".into()),
            ("PYTHONDONTWRITEBYTECODE".into(), "1".into()),
        ],
        capture_limit: 4096,
    };
    if partial {
        spec.args.push("partial".into());
    }
    let child =
        RetainedOwnedChild::spawn(spec, &MacIdentity, Instant::now() + Duration::from_secs(3))
            .unwrap();
    let result = actors::Original::new(child).collect(Instant::now() + Duration::from_secs(32));
    assert!(result.complete && result.exit == ProcessExit::Code(0));
    assert!(result.stderr.is_empty());
    result.stdout
}

fn require_actual_rows(raw: &[u8]) {
    let text = std::str::from_utf8(raw).unwrap();
    let mut lines = text.lines();
    let peers = lines
        .next()
        .unwrap()
        .strip_prefix("PEERS\t")
        .unwrap()
        .split('\t')
        .map(|port| port.parse::<u16>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(peers.len(), 4);
    assert_eq!(lines.next(), Some("OVERLAP\tverified-two-before-data"));
    let metadata = format!("{}\n", lines.collect::<Vec<_>>().join("\n"));
    let verdict = codec::parse_wave(metadata.as_bytes()).unwrap();
    assert_eq!(verdict.sessions.len(), 4);
    for (index, port) in peers.iter().enumerate() {
        let session = verdict
            .sessions
            .iter()
            .find(|session| session.ordinal == index + 1)
            .unwrap();
        assert_eq!(session.peer_port, *port);
        assert!(session.closed);
        let expected = match index {
            0 | 1 => (
                codec::Phase::ApplicationRead,
                codec::Category::ApplicationData,
            ),
            2 => (
                codec::Phase::Handshake,
                codec::Category::CertificateAlertUnknownCa,
            ),
            _ => (codec::Phase::Handshake, codec::Category::TransportEof),
        };
        assert_eq!((session.phase, session.category), expected);
    }
    let upstreams = peers
        .iter()
        .enumerate()
        .map(|(index, port)| (index + 1, ([127, 0, 0, 1], *port).into()))
        .collect::<Vec<_>>();
    assert!(!codec::bound_wave_rejection(&verdict, &upstreams));
    assert!(codec::parse(metadata.as_bytes()).is_err());
}
