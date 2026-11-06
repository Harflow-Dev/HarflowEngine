use super::*;
use hf_platform::qualification::live::TwoTunnelOutcome;
use std::collections::BTreeSet;
use std::io::ErrorKind;

pub(super) fn preflight(proxy: &TwoTunnelProxy) {
    let mut probe = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    probe.write_all(PREFLIGHT).unwrap();
    let reply = read_to_end(&mut probe);
    assert!(reply.windows(12).any(|part| part == b"preflight-ok"));
}

pub(super) fn tunnel(port: u16) -> TcpStream {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .write_all(b"CONNECT api.anthropic.com:443 HTTP/1.1\r\nHost: api.anthropic.com:443\r\n\r\n")
        .unwrap();
    stream
}

pub(super) fn established(streams: &mut [TcpStream; 2]) {
    for stream in streams {
        established_one(stream);
    }
}

pub(super) fn established_one(stream: &mut TcpStream) {
    let mut response = Vec::new();
    let mut byte = [0];
    while !response.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        response.push(byte[0]);
    }
    assert!(response.starts_with(b"HTTP/1.1 200 Connection Established"));
}

pub(super) fn finish_pair(streams: &mut [TcpStream; 2], ids: [u8; 2]) {
    for (stream, id) in streams.iter_mut().zip(ids) {
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        assert_eq!(response, [id]);
    }
}

pub(super) fn finish_one(stream: &mut TcpStream, id: u8) {
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    assert_eq!(response, [id]);
}

pub(super) fn serve_four(listener: TcpListener) {
    for _ in 0..4 {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.len(), 1);
        stream.write_all(&bytes).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
    }
}

pub(super) fn accept_and_close_two() -> (SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target = listener.local_addr().unwrap();
    let helper = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).unwrap();
        }
    });
    (target, helper)
}

pub(super) fn assert_four_settled(result: &TwoTunnelOutcome, target: SocketAddr) {
    assert!(!result.failed && result.owned_sockets_closed && result.worker_joined);
    assert_eq!(
        (result.preflights, result.accepted, result.connects),
        (1, 4, 4)
    );
    assert_eq!(result.slots.len(), 4);
    assert_eq!(
        result
            .slots
            .iter()
            .map(|slot| slot.ordinal)
            .collect::<BTreeSet<_>>(),
        [1, 2, 3, 4].into_iter().collect()
    );
    for slot in &result.slots {
        assert_eq!(slot.upstream_peer, Some(target));
        assert!(slot.client_to_upstream.eof && slot.client_to_upstream.write_half_closed);
        assert!(slot.upstream_to_client.eof && slot.upstream_to_client.write_half_closed);
        assert!(slot.client_closed && slot.upstream_closed);
        assert!(slot.client_to_upstream.bytes_forwarded > 0);
        assert!(slot.upstream_to_client.bytes_forwarded > 0);
    }
    let locals = result
        .slots
        .iter()
        .map(|slot| slot.upstream_local.unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(locals.len(), 4);
}

fn read_to_end(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    bytes
}

pub(super) fn assert_closed_unread(stream: &mut TcpStream) {
    let mut byte = [0];
    match stream.read(&mut byte) {
        Ok(0) => (),
        Err(error) => assert_eq!(error.kind(), ErrorKind::ConnectionReset),
        Ok(_) => panic!("rejected socket returned payload"),
    }
}
