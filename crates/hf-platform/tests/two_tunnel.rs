#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_platform::qualification::live::{TwoTunnelFailureReason, TwoTunnelProxy};
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[path = "two_tunnel_support/budgets.rs"]
mod budgets;
#[path = "two_tunnel_support/chronology.rs"]
mod chronology;
#[path = "two_tunnel_support/protocol.rs"]
mod protocol;
mod two_tunnel_support;

const NONCE: &str = "a1b2c3d4e5f60718";
const PREFLIGHT: &[u8] =
    b"GET /a1b2c3d4e5f60718/preflight HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
const CONNECT: &[u8] =
    b"CONNECT api.anthropic.com:443 HTTP/1.1\r\nHost: api.anthropic.com:443\r\n\r\n";

#[test]
fn two_opaque_tunnels_relay_independently_and_settle() {
    let (target, seen_rx, helper) = start_two_stream_helper();
    let proxy =
        TwoTunnelProxy::start(target, NONCE, Instant::now() + Duration::from_secs(8)).unwrap();
    let mut probe = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    probe.write_all(PREFLIGHT).unwrap();
    let preflight = two_tunnel_support::read_to_end(&mut probe);
    assert!(
        preflight
            .windows(12)
            .any(|window| window == b"preflight-ok")
    );
    let replies = exchange_two_streams(proxy.port());
    assert_eq!(replies[0], b"opaque-return-alpha");
    assert_eq!(replies[1], b"opaque-return-beta");
    let mut observed = [
        seen_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        seen_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
    ];
    observed.sort();
    assert_eq!(
        observed,
        [
            b"\x01opaque-stream-alpha".to_vec(),
            b"\x02opaque-stream-beta".to_vec()
        ]
    );
    helper.join().unwrap();
    let ready_until = Instant::now() + Duration::from_secs(2);
    while !proxy.observation_ready() && Instant::now() < ready_until {
        thread::yield_now();
    }
    assert!(
        proxy.observation_ready(),
        "clean tunnel settlement was not observed"
    );
    assert_clean_two_stream_outcome(proxy, target);
}

fn start_two_stream_helper() -> (SocketAddr, Receiver<Vec<u8>>, JoinHandle<()>) {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target = upstream.local_addr().unwrap();
    let (seen_tx, seen_rx) = mpsc::channel();
    let helper = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = upstream.accept().unwrap();
            let mut received = [0; 21];
            stream.read_exact(&mut received[..1]).unwrap();
            let size = match received[0] {
                1 => b"opaque-stream-alpha".len(),
                2 => b"opaque-stream-beta".len(),
                _ => panic!("unknown opaque stream identity"),
            } + 1;
            stream.read_exact(&mut received[1..size]).unwrap();
            let received = received[..size].to_vec();
            let response = if received[1..] == *b"opaque-stream-alpha" {
                b"opaque-return-alpha".as_slice()
            } else {
                b"opaque-return-beta".as_slice()
            };
            stream.write_all(response).unwrap();
            seen_tx.send(received).unwrap();
        }
    });
    (target, seen_rx, helper)
}

fn exchange_two_streams(port: u16) -> [Vec<u8>; 2] {
    let mut clients = [
        TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap(),
        TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap(),
    ];
    for client in &mut clients {
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client.write_all(CONNECT).unwrap();
    }
    for client in &mut clients {
        let response = two_tunnel_support::read_headers(client);
        assert!(response.starts_with(b"HTTP/1.1 200 Connection Established"));
    }
    clients[0].write_all(b"\x01opaque-stream-alpha").unwrap();
    clients[1].write_all(b"\x02opaque-stream-beta").unwrap();
    clients[0].shutdown(Shutdown::Write).unwrap();
    clients[1].shutdown(Shutdown::Write).unwrap();
    let first = two_tunnel_support::read_to_end(&mut clients[0]);
    let second = two_tunnel_support::read_to_end(&mut clients[1]);
    [first, second]
}

fn assert_clean_two_stream_outcome(proxy: TwoTunnelProxy, target: SocketAddr) {
    let result = proxy.stop_and_join().unwrap();
    assert_eq!(result.preflights, 1);
    assert_eq!(result.accepted, 2);
    assert_eq!(result.connects, 2);
    assert!(!result.failed);
    assert!(result.owned_sockets_closed && result.worker_joined);
    assert_eq!(result.slots.len(), 2);
    assert!(
        result
            .slots
            .iter()
            .all(|slot| slot.client_closed && slot.upstream_closed)
    );
    assert!(
        result
            .slots
            .iter()
            .all(|slot| slot.client_to_upstream.bytes_forwarded > 0)
    );
    assert!(
        result
            .slots
            .iter()
            .all(|slot| slot.upstream_to_client.bytes_forwarded > 0)
    );
    assert_eq!(
        result
            .slots
            .iter()
            .map(|slot| slot.ordinal)
            .collect::<std::collections::BTreeSet<_>>(),
        [1, 2].into_iter().collect()
    );
    assert!(
        result
            .slots
            .iter()
            .all(|slot| slot.upstream_peer == Some(target))
    );
    assert_ne!(
        result.slots[0].upstream_local,
        result.slots[1].upstream_local
    );
}

#[test]
fn third_post_preflight_socket_poison_closes_both_active_tunnels() {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target = upstream.local_addr().unwrap();
    let (seen_tx, seen_rx) = mpsc::channel();
    let helper = thread::spawn(move || {
        let mut sockets = [upstream.accept().unwrap().0, upstream.accept().unwrap().0];
        for socket in &mut sockets {
            let mut initial = [0; 1];
            socket.read_exact(&mut initial).unwrap();
            seen_tx.send(initial[0]).unwrap();
        }
        let mut remainder = Vec::new();
        for socket in &mut sockets {
            let mut bytes = Vec::new();
            socket.read_to_end(&mut bytes).unwrap();
            remainder.extend(bytes);
        }
        remainder
    });
    let proxy =
        TwoTunnelProxy::start(target, NONCE, Instant::now() + Duration::from_secs(5)).unwrap();
    let mut probe = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    probe.write_all(PREFLIGHT).unwrap();
    let _ = two_tunnel_support::read_to_end(&mut probe);
    let mut clients = [
        two_tunnel_support::connect_tunnel(proxy.port()),
        two_tunnel_support::connect_tunnel(proxy.port()),
    ];
    clients[0].write_all(b"A").unwrap();
    clients[1].write_all(b"B").unwrap();
    let mut witnessed = [
        seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
    ];
    witnessed.sort();
    assert_eq!(witnessed, [b'A', b'B']);
    let mut third = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    third
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut byte = [0];
    assert_eq!(third.read(&mut byte).unwrap(), 0);
    for client in &mut clients {
        let _ = client.write_all(b"AFTER-POISON");
    }
    drop(clients);
    let remainder = helper.join().unwrap();
    assert!(remainder.is_empty(), "post-poison bytes reached upstream");
    let result = proxy.stop_and_join().unwrap();
    assert!(result.failed && result.owned_sockets_closed && result.worker_joined);
    assert_eq!(result.accepted, 3);
    assert_eq!(
        result.first_failure.unwrap().reason,
        TwoTunnelFailureReason::ExcessConnection
    );
    assert_eq!(result.connects, 2);
    assert!(
        result
            .slots
            .iter()
            .all(|slot| slot.client_closed && slot.upstream_closed)
    );
}

#[test]
fn invalid_routes_poison_active_tunnel_without_followup_forwarding() {
    let cases: [(&[u8], bool); 7] = [
        (b"CONNECT\r\n\r\n", false),
        (b"CONNECT api.anthropic.com:443 HTTP/1.1\r\nHost: api.anthropic.com:443\r\nX: y\r\n\r\n", false),
        (b"CONNECT api.anthropic.com:443 HTTP/1.1\r\nHost: api.anthropic.com:443\r\nHost: api.anthropic.com:443\r\n\r\n", false),
        (b"CONNECT api.anthropic.com:443 HTTP/1.1\r\nHost: api.anthropic.com:443\r\nAuthorization: Bearer fake\r\n\r\n", false),
        (b"CONNECT api.anthropic.com:443 HTTP/1.1\r\nHost: wrong.example:443\r\n\r\n", false),
        (b"CONNECT other.example:443 HTTP/1.1\r\nHost: other.example:443\r\n\r\n", false),
        (PREFLIGHT, true),
    ];
    for (request, is_preflight) in cases {
        protocol::assert_invalid_route(request, is_preflight);
    }
}

#[test]
fn ipv6_attempt_poison_is_observed_and_never_opens_upstream() {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target = upstream.local_addr().unwrap();
    let monitor = upstream.try_clone().unwrap();
    let (seen_tx, seen_rx) = mpsc::channel();
    let helper = thread::spawn(move || {
        let mut sockets = [upstream.accept().unwrap().0, upstream.accept().unwrap().0];
        for socket in &mut sockets {
            let mut initial = [0; 1];
            socket.read_exact(&mut initial).unwrap();
            seen_tx.send(initial[0]).unwrap();
        }
        let mut remainder = Vec::new();
        for socket in &mut sockets {
            let mut bytes = Vec::new();
            socket.read_to_end(&mut bytes).unwrap();
            remainder.extend(bytes);
        }
        remainder
    });
    let proxy =
        TwoTunnelProxy::start(target, NONCE, Instant::now() + Duration::from_secs(3)).unwrap();
    let mut probe = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    probe.write_all(PREFLIGHT).unwrap();
    let _ = two_tunnel_support::read_to_end(&mut probe);
    let mut clients = [
        two_tunnel_support::connect_tunnel(proxy.port()),
        two_tunnel_support::connect_tunnel(proxy.port()),
    ];
    clients[0].write_all(b"A").unwrap();
    clients[1].write_all(b"B").unwrap();
    let mut witnessed = [
        seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
    ];
    witnessed.sort();
    assert_eq!(witnessed, [b'A', b'B']);
    let mut ipv6 = TcpStream::connect((std::net::Ipv6Addr::LOCALHOST, proxy.port())).unwrap();
    ipv6.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    let mut byte = [0];
    assert_eq!(ipv6.read(&mut byte).unwrap(), 0);
    for client in &mut clients {
        let _ = client.write_all(b"AFTER-POISON");
    }
    drop(clients);
    let remainder = helper.join().unwrap();
    assert!(remainder.is_empty(), "post-poison bytes reached upstream");
    monitor.set_nonblocking(true).unwrap();
    assert!(matches!(
        monitor.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    let result = proxy.stop_and_join().unwrap();
    assert_eq!(result.ipv6_connections, 1);
    assert_eq!(
        result.first_failure.unwrap().reason,
        TwoTunnelFailureReason::Ipv6
    );
    assert_eq!(result.connects, 2);
    assert!(result.failed && result.owned_sockets_closed && result.worker_joined);
    assert!(
        result
            .slots
            .iter()
            .all(|slot| slot.client_closed && slot.upstream_closed)
    );
}

#[test]
fn closed_slot_does_not_recycle_the_two_connection_allowance() {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target = upstream.local_addr().unwrap();
    let helper = thread::spawn(move || {
        let first = upstream.accept().unwrap().0;
        let mut second = upstream.accept().unwrap().0;
        first.shutdown(Shutdown::Both).unwrap();
        let mut scratch = [0; 8];
        let _ = second.read(&mut scratch);
    });
    let proxy =
        TwoTunnelProxy::start(target, NONCE, Instant::now() + Duration::from_secs(5)).unwrap();
    let mut probe = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    probe.write_all(PREFLIGHT).unwrap();
    let _ = two_tunnel_support::read_to_end(&mut probe);
    let first = two_tunnel_support::connect_tunnel(proxy.port());
    let second = two_tunnel_support::connect_tunnel(proxy.port());
    drop(first);
    let mut third = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    third
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut byte = [0];
    assert_eq!(third.read(&mut byte).unwrap(), 0);
    drop(second);
    helper.join().unwrap();
    let result = proxy.stop_and_join().unwrap();
    assert_eq!(result.accepted, 3);
    assert_eq!(result.connects, 2);
    assert_eq!(
        result.first_failure.unwrap().reason,
        TwoTunnelFailureReason::ExcessConnection
    );
}

#[test]
fn partial_second_header_does_not_block_first_tunnel_or_reset_deadline() {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target = upstream.local_addr().unwrap();
    let helper = thread::spawn(move || {
        let (mut socket, _) = upstream.accept().unwrap();
        let mut request = [0; 1];
        socket.read_exact(&mut request).unwrap();
        socket.write_all(b"R").unwrap();
        let mut discard = Vec::new();
        socket.read_to_end(&mut discard).unwrap();
    });
    let until = Instant::now() + Duration::from_millis(600);
    let proxy = TwoTunnelProxy::start(target, NONCE, until).unwrap();
    let mut probe = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    probe.write_all(PREFLIGHT).unwrap();
    let _ = two_tunnel_support::read_to_end(&mut probe);
    let mut first = two_tunnel_support::connect_tunnel(proxy.port());
    let mut partial = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    partial.write_all(b"CONNECT api.anthropic.com:").unwrap();
    first.write_all(b"Q").unwrap();
    first
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut response = [0; 1];
    first.read_exact(&mut response).unwrap();
    assert_eq!(response, *b"R");
    let mut closed = Vec::new();
    first.read_to_end(&mut closed).unwrap();
    helper.join().unwrap();
    let result = proxy.stop_and_join().unwrap();
    assert_eq!(
        result.first_failure.unwrap().reason,
        TwoTunnelFailureReason::Deadline
    );
    assert!(result.failed && result.owned_sockets_closed);
    assert!(
        result
            .slots
            .iter()
            .any(|slot| slot.client_to_upstream.bytes_forwarded == 1)
    );
    drop(partial);
}
