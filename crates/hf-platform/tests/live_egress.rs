#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_platform::qualification::{
    FixtureRoot,
    live::{
        OriginalActorsSettled, Proxy, ProxyMode, live_profile, remove_matched_files, scan_secret,
    },
};
use std::{
    fs,
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const NONCE: &str = "a1b2c3d4e5f60718";

#[test]
fn live_profile_adds_only_the_credentials_file_write_denial() {
    let root = FixtureRoot::create().unwrap();
    let base = hf_platform::qualification::recorded_vendor_profile(
        &root.recorded_topology().unwrap(),
        43127,
    )
    .unwrap();
    let profile = live_profile(&root, 43127).unwrap();
    assert!(profile.starts_with(&base));
    assert!(profile.ends_with("(deny file-write* (regex #\"(^|/)[.]credentials[.]json$\"))\n"));
    assert_eq!(profile.matches("(deny file-write* (regex").count(), 1);
}

#[test]
fn secret_scan_is_bounded_to_the_owned_root_and_removal_requires_settlement() {
    let root = FixtureRoot::create().unwrap();
    let canary = root.runtime().join("capture.bin");
    fs::write(&canary, b"prefix opaque-token-value suffix").unwrap();
    let matches = scan_secret(
        &root,
        b"opaque-token-value",
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(matches.count(), 1);
    let settled = OriginalActorsSettled::attest_original_waits_eof_and_joins();
    assert_eq!(
        remove_matched_files(
            &root,
            matches,
            &settled,
            Instant::now() + Duration::from_secs(2)
        )
        .unwrap(),
        1
    );
    assert!(!canary.exists());
    assert!(
        scan_secret(
            &root,
            b"opaque-token-value",
            Instant::now() + Duration::from_secs(2)
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn proxy_relays_exact_opaque_bytes_and_completes_both_half_closes() {
    let upstream = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let (sender, receiver) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut socket, _) = upstream.accept().unwrap();
        let mut received = Vec::new();
        socket.read_to_end(&mut received).unwrap();
        socket.write_all(b"remote-response").unwrap();
        drop(socket);
        sender.send(received).unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let proxy = Proxy::start(ProxyMode::UntrustedTls(upstream_addr), NONCE, deadline).unwrap();
    let mut probe = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
    probe
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    probe
        .write_all(
            format!(
                "GET /{NONCE}/preflight HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
    let mut probe_response = Vec::new();
    probe.read_to_end(&mut probe_response).unwrap();
    assert!(
        probe_response
            .windows(b"preflight-ok".len())
            .any(|w| w == b"preflight-ok")
    );

    let mut client = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .write_all(b"CONNECT api.anthropic.com:443 HTTP/1.1\r\nHost: api.anthropic.com:443\r\n\r\n")
        .unwrap();
    let mut response = [0; 39];
    client.read_exact(&mut response).unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200 Connection Established"));
    let opaque = b"client-opaque-tls-record";
    client.write_all(opaque).unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut remote_response = Vec::new();
    client.read_to_end(&mut remote_response).unwrap();
    assert!(
        remote_response
            .windows(b"remote-response".len())
            .any(|w| w == b"remote-response")
    );
    let forwarded = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(forwarded, opaque);
    server.join().unwrap();
    let outcome = proxy.stop_and_join().unwrap();
    assert_eq!(outcome.preflights, 1);
    assert_eq!(outcome.connects, 1);
    assert!(outcome.client_eof && outcome.remote_eof && outcome.client_to_upstream_flushed);
    assert!(outcome.owned_sockets_closed && !outcome.failed);
}

#[test]
fn proxy_poison_closes_after_an_invalid_route_without_a_second_connect() {
    let upstream = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = upstream.local_addr().unwrap();
    let proxy = Proxy::start(
        ProxyMode::UntrustedTls(address),
        NONCE,
        Instant::now() + Duration::from_secs(3),
    )
    .unwrap();
    let mut client = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
    client
        .write_all(b"GET /wrong HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
        .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).unwrap();
    let outcome = proxy.stop_and_join().unwrap();
    assert!(outcome.failed && outcome.owned_sockets_closed);
    assert_eq!(outcome.preflights, 0);
    assert_eq!(outcome.connects, 0);
    assert!(outcome.rejected_connections <= 1);
}

#[test]
fn proxy_rejects_invalid_nonce_and_nonloopback_negative_upstream_before_binding() {
    let deadline = Instant::now() + std::time::Duration::from_secs(1);
    assert!(Proxy::start(ProxyMode::Reject, "bad nonce", deadline).is_err());
    assert!(Proxy::start(ProxyMode::Reject, NONCE, Instant::now()).is_err());
    assert!(
        Proxy::start(
            ProxyMode::UntrustedTls("192.0.2.1:443".parse().unwrap()),
            NONCE,
            deadline,
        )
        .is_err()
    );
}
