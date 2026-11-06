#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_platform::qualification::live::{TwoTunnelFailureReason, TwoTunnelProxy};
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};

#[path = "replacement_wave_support/budgets.rs"]
mod budgets;
#[path = "replacement_wave_support/fixtures.rs"]
mod fixtures;

const NONCE: &str = "wavea1b2c3d4e5f60718";
const PREFLIGHT: &[u8] =
    b"GET /wavea1b2c3d4e5f60718/preflight HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";

#[test]
fn two_overlapping_streams_retire_before_two_replacements() {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target = upstream.local_addr().unwrap();
    let helper = thread::spawn(move || fixtures::serve_four(upstream));
    let proxy = TwoTunnelProxy::start_replacement_wave(
        target,
        NONCE,
        Instant::now() + Duration::from_secs(8),
    )
    .unwrap();
    fixtures::preflight(&proxy);

    let mut first = [
        fixtures::tunnel(proxy.port()),
        fixtures::tunnel(proxy.port()),
    ];
    fixtures::established(&mut first);
    for (index, stream) in first.iter_mut().enumerate() {
        stream.write_all(&[b'A' + index as u8]).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
    }
    fixtures::finish_pair(&mut first, *b"AB");

    for id in *b"CD" {
        let mut stream = fixtures::tunnel(proxy.port());
        fixtures::established_one(&mut stream);
        stream.write_all(&[id]).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        fixtures::finish_one(&mut stream, id);
    }
    helper.join().unwrap();
    let result = proxy.stop_and_join().unwrap();
    fixtures::assert_four_settled(&result, target);
    assert_eq!(result.peak_active_slots, 2);
    assert_eq!(result.retired_slots_closed, 4);
}

#[test]
fn third_concurrent_slot_is_rejected_without_reading_its_request() {
    let (target, helper) = fixtures::accept_and_close_two();
    let proxy = TwoTunnelProxy::start_replacement_wave(
        target,
        NONCE,
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
    fixtures::preflight(&proxy);
    let mut clients = [
        fixtures::tunnel(proxy.port()),
        fixtures::tunnel(proxy.port()),
    ];
    fixtures::established(&mut clients);
    let mut third = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    third
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    third.write_all(b"MUST-REMAIN-UNREAD").unwrap();
    fixtures::assert_closed_unread(&mut third);
    drop(clients);
    helper.join().unwrap();
    let result = proxy.stop_and_join().unwrap();
    assert!(result.failed && result.owned_sockets_closed && result.worker_joined);
    assert_eq!(
        result.first_failure.unwrap().reason,
        TwoTunnelFailureReason::ExcessConnection
    );
    assert_eq!(result.accepted, 3);
    assert_eq!(result.connects, 2);
}

#[test]
fn fifth_total_slot_is_rejected_after_two_retirements() {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target = upstream.local_addr().unwrap();
    let helper = thread::spawn(move || fixtures::serve_four(upstream));
    let proxy = TwoTunnelProxy::start_replacement_wave(
        target,
        NONCE,
        Instant::now() + Duration::from_secs(8),
    )
    .unwrap();
    fixtures::preflight(&proxy);
    for id in b"ABCD" {
        let mut stream = fixtures::tunnel(proxy.port());
        fixtures::established_one(&mut stream);
        stream.write_all(&[*id]).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        fixtures::finish_one(&mut stream, *id);
    }
    let mut fifth = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    fifth
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    fifth.write_all(b"MUST-REMAIN-UNREAD").unwrap();
    fixtures::assert_closed_unread(&mut fifth);
    helper.join().unwrap();
    let result = proxy.stop_and_join().unwrap();
    assert!(result.failed && result.owned_sockets_closed && result.worker_joined);
    assert_eq!(
        result.first_failure.unwrap().reason,
        TwoTunnelFailureReason::ExcessConnection
    );
    assert_eq!(result.accepted, 5);
    assert_eq!(result.connects, 4);
    assert_eq!(result.retired_slots_closed, 4);
}

#[test]
fn invalid_route_globally_poisons_without_forwarding_trailing_bytes() {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target = upstream.local_addr().unwrap();
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let helper = thread::spawn(move || {
        let (mut stream, _) = upstream.accept().unwrap();
        let mut first = [0];
        stream.read_exact(&mut first).unwrap();
        seen_tx.send(first[0]).unwrap();
        let mut remainder = Vec::new();
        stream.read_to_end(&mut remainder).unwrap();
        remainder
    });
    let proxy = TwoTunnelProxy::start_replacement_wave(
        target,
        NONCE,
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
    fixtures::preflight(&proxy);
    let mut active = fixtures::tunnel(proxy.port());
    fixtures::established_one(&mut active);
    active.write_all(b"A").unwrap();
    assert_eq!(seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(), b'A');

    let mut invalid = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    invalid
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    invalid
        .write_all(
            b"CONNECT api.anthropic.com:443 HTTP/1.1\r\nHost: wrong.example:443\r\n\r\nUNREAD",
        )
        .unwrap();
    fixtures::assert_closed_unread(&mut invalid);
    let _ = active.write_all(b"AFTER-POISON");
    drop(active);
    assert!(helper.join().unwrap().is_empty());
    let result = proxy.stop_and_join().unwrap();
    assert_eq!(
        result.first_failure.unwrap().reason,
        TwoTunnelFailureReason::Route
    );
    assert_eq!(result.connects, 1);
    assert!(result.failed && result.owned_sockets_closed && result.worker_joined);
}

#[test]
fn ipv6_connection_globally_poisons_both_active_tunnels() {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target = upstream.local_addr().unwrap();
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let helper = thread::spawn(move || {
        let mut streams = [upstream.accept().unwrap().0, upstream.accept().unwrap().0];
        for stream in &mut streams {
            let mut first = [0];
            stream.read_exact(&mut first).unwrap();
            seen_tx.send(first[0]).unwrap();
        }
        streams
            .into_iter()
            .map(|mut stream| {
                let mut rest = Vec::new();
                stream.read_to_end(&mut rest).unwrap();
                rest
            })
            .collect::<Vec<_>>()
    });
    let proxy = TwoTunnelProxy::start_replacement_wave(
        target,
        NONCE,
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
    fixtures::preflight(&proxy);
    let mut clients = [
        fixtures::tunnel(proxy.port()),
        fixtures::tunnel(proxy.port()),
    ];
    fixtures::established(&mut clients);
    clients[0].write_all(b"A").unwrap();
    clients[1].write_all(b"B").unwrap();
    let mut seen = [
        seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
    ];
    seen.sort();
    assert_eq!(seen, *b"AB");
    let mut ipv6 = TcpStream::connect((Ipv6Addr::LOCALHOST, proxy.port())).unwrap();
    ipv6.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    ipv6.write_all(b"MUST-REMAIN-UNREAD").unwrap();
    fixtures::assert_closed_unread(&mut ipv6);
    drop(clients);
    let remainders = helper.join().unwrap();
    assert!(remainders.iter().all(Vec::is_empty));
    let result = proxy.stop_and_join().unwrap();
    assert_eq!(
        result.first_failure.unwrap().reason,
        TwoTunnelFailureReason::Ipv6
    );
    assert_eq!(result.ipv6_connections, 1);
    assert_eq!(result.connects, 2);
    assert!(result.failed && result.owned_sockets_closed && result.worker_joined);
}
