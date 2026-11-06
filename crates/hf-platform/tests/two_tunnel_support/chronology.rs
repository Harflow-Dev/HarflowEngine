use super::*;

#[test]
fn settled_eof_chronology_precedes_third_connection_poison() {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target = upstream.local_addr().unwrap();
    let helper = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = upstream.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut payload = [0; 1];
            stream.read_exact(&mut payload).unwrap();
            assert!(stream.read(&mut [0]).unwrap() == 0);
            stream.write_all(&payload).unwrap();
        }
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
    for (index, client) in clients.iter_mut().enumerate() {
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client.write_all(&[b'A' + index as u8]).unwrap();
        client.shutdown(Shutdown::Write).unwrap();
    }
    for (index, client) in clients.iter_mut().enumerate() {
        let mut echoed = [0];
        client.read_exact(&mut echoed).unwrap();
        assert_eq!(echoed, [b'A' + index as u8]);
        assert_eq!(client.read(&mut [0]).unwrap(), 0);
    }
    helper.join().unwrap();
    let settled_by = Instant::now() + Duration::from_secs(2);
    while !proxy.observation_ready() && Instant::now() < settled_by {
        thread::yield_now();
    }
    assert!(
        proxy.observation_ready(),
        "clean EOF was not observed before third connection"
    );
    let mut third = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    third
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    assert_eq!(third.read(&mut [0]).unwrap(), 0);
    let result = proxy.stop_and_join().unwrap();
    assert_eq!(result.accepted, 3);
    assert_eq!(result.connects, 2);
    assert_eq!(
        result.first_failure.unwrap().reason,
        TwoTunnelFailureReason::ExcessConnection
    );
    assert!(result.failed && result.owned_sockets_closed && result.worker_joined);
    assert_eq!(result.slots.len(), 2);
    for slot in result.slots {
        assert!(slot.client_to_upstream.eof);
        assert!(slot.client_to_upstream.write_half_closed);
        assert!(slot.upstream_to_client.eof);
        assert!(slot.upstream_to_client.write_half_closed);
        assert!(slot.client_closed && slot.upstream_closed);
    }
}
