use super::*;

pub(super) fn run() {
    let root = FixtureRoot::create().expect("isolated fixture root");
    let helper = start_helper(&root, "curl");
    let proxy = start_proxy(helper.address);
    preflight(&proxy);
    let spec = SpawnSpec {
        executable: fs::canonicalize("/usr/bin/curl").expect("system curl path"),
        cwd: std::env::current_dir().expect("test working directory"),
        args: vec![
            "--disable".into(),
            "--silent".into(),
            "--proxy".into(),
            format!("http://127.0.0.1:{}", proxy.port()).into(),
            "--noproxy".into(),
            "".into(),
            "--max-time".into(),
            "8".into(),
            "--connect-timeout".into(),
            "3".into(),
            "--max-redirs".into(),
            "0".into(),
            "--output".into(),
            "/dev/null".into(),
            "https://api.anthropic.com/".into(),
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
            .unwrap();
    let curl = actors::Original::new(child).collect(Instant::now() + Duration::from_secs(8));
    assert!(curl.complete && curl.exit == ProcessExit::Code(60));
    assert!(curl.stdout.is_empty() && curl.stderr.is_empty());
    let mut eof = TcpStream::connect(helper.address).unwrap();
    let eof_port = eof.local_addr().unwrap().port();
    eof.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    eof.shutdown(Shutdown::Write).unwrap();
    let _ = std::io::Read::read(&mut eof, &mut [0]);
    let outcome = joined_proxy(proxy);
    let (address, verdict) = finish_helper(helper);
    verify_curl_metadata(&outcome, address, &verdict, eof_port);
}

fn verify_curl_metadata(
    outcome: &hf_platform::qualification::live::TwoTunnelOutcome,
    address: SocketAddr,
    verdict: &codec::Verdict,
    eof_port: u16,
) {
    eprintln!(
        "curl-local exit=60 proxy-failed={} reason={:?} kind={:?} accepted={} connects={} slots={:?} helper={:?}",
        outcome.failed,
        outcome.first_failure.as_ref().map(|failure| failure.reason),
        outcome
            .first_failure
            .as_ref()
            .and_then(|failure| failure.io_kind),
        outcome.accepted,
        outcome.connects,
        outcome
            .slots
            .iter()
            .map(|slot| (
                slot.ordinal,
                slot.client_to_upstream.eof,
                slot.client_to_upstream.write_half_closed,
                slot.upstream_to_client.eof,
                slot.upstream_to_client.write_half_closed
            ))
            .collect::<Vec<_>>(),
        verdict
            .sessions
            .iter()
            .map(|session| (
                session.ordinal,
                session.status,
                session.phase,
                session.category
            ))
            .collect::<Vec<_>>()
    );
    assert!(outcome.failed);
    assert_eq!(
        outcome.first_failure.as_ref().unwrap().reason,
        TwoTunnelFailureReason::Io
    );
    // Native closure timing may yield NotConnected or ConnectionReset. The
    // contract is preserved native detail, never a synthesized graceful EOF.
    assert!(outcome.first_failure.as_ref().unwrap().io_kind.is_some());
    assert_eq!(outcome.preflights, 1);
    assert_eq!(outcome.accepted, 1);
    assert_eq!(outcome.connects, 1);
    assert!(outcome.owned_sockets_closed && outcome.worker_joined);
    assert_eq!(outcome.slots.len(), 1);
    let curl_session = verdict
        .sessions
        .iter()
        .find(|session| session.ordinal == 1)
        .unwrap();
    let eof_session = verdict
        .sessions
        .iter()
        .find(|session| session.ordinal == 2)
        .unwrap();
    assert_eq!(
        curl_session.category,
        codec::Category::CertificateAlertUnknownCa
    );
    assert_eq!(curl_session.phase, codec::Phase::Handshake);
    assert_eq!(eof_session.category, codec::Category::TransportEof);
    assert_eq!(eof_session.phase, codec::Phase::Handshake);
    assert_eq!(eof_session.peer_port, eof_port);
    assert_eq!(outcome.slots[0].upstream_peer, Some(address));
    assert_eq!(
        outcome.slots[0].upstream_local.unwrap().port(),
        curl_session.peer_port
    );
    assert!(outcome.slots[0].client_closed && outcome.slots[0].upstream_closed);
    for direction in [
        &outcome.slots[0].client_to_upstream,
        &outcome.slots[0].upstream_to_client,
    ] {
        assert!(direction.bytes_forwarded <= direction.bytes_read);
        assert_eq!(
            direction.buffered_bytes,
            direction.bytes_read - direction.bytes_forwarded
        );
        assert!(!direction.write_half_closed || direction.eof);
        assert!(!direction.write_half_closed || direction.buffered_bytes == 0);
    }
}
