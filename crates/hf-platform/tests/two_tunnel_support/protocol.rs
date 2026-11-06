use super::*;
use std::{io::ErrorKind, net::SocketAddr};

pub(super) fn assert_invalid_route(request: &[u8], duplicate_preflight: bool) {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target: SocketAddr = upstream.local_addr().unwrap();
    let probe_listener = upstream.try_clone().unwrap();
    let (seen_tx, seen_rx) = std::sync::mpsc::channel();
    let receiver = std::thread::spawn(move || {
        let (mut socket, _) = upstream.accept().unwrap();
        let mut before = [0; 1];
        socket.read_exact(&mut before).unwrap();
        seen_tx.send(before[0]).unwrap();
        let mut remainder = Vec::new();
        socket.read_to_end(&mut remainder).unwrap();
        remainder
    });
    let proxy =
        TwoTunnelProxy::start(target, NONCE, Instant::now() + Duration::from_secs(3)).unwrap();
    let mut probe = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    probe.write_all(PREFLIGHT).unwrap();
    let _ = two_tunnel_support::read_to_end(&mut probe);
    let mut active = two_tunnel_support::connect_tunnel(proxy.port());
    active.write_all(b"B").unwrap();
    assert_eq!(seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(), b'B');
    let mut invalid = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    invalid
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    invalid.write_all(request).unwrap();
    let mut response = Vec::new();
    let _ = invalid.read_to_end(&mut response);
    let _ = active.write_all(b"AFTER-POISON");
    drop(active);
    let remainder = receiver.join().unwrap();
    let result = proxy.stop_and_join().unwrap();
    assert!(result.failed && result.owned_sockets_closed);
    assert_eq!(result.connects, 1);
    assert!(remainder.is_empty(), "post-poison bytes reached upstream");
    probe_listener.set_nonblocking(true).unwrap();
    assert!(matches!(probe_listener.accept(), Err(error) if error.kind() == ErrorKind::WouldBlock));
    if duplicate_preflight {
        assert_eq!(
            result.first_failure.unwrap().reason,
            TwoTunnelFailureReason::Preflight
        );
    }
}
