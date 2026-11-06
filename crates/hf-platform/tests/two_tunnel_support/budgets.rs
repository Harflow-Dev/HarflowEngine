use super::*;

#[test]
fn client_to_upstream_budget_and_pending_buffers_are_shared() {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target = upstream.local_addr().unwrap();
    let (release_tx, release_rx) = mpsc::channel();
    let helper = thread::spawn(move || {
        let mut sockets = [upstream.accept().unwrap().0, upstream.accept().unwrap().0];
        release_rx.recv().unwrap();
        for socket in &mut sockets {
            let mut bytes = [0; 32768];
            while socket.read(&mut bytes).unwrap_or(0) > 0 {}
        }
    });
    let (proxy, clients) = open_two_tunnels(target);
    let senders: Vec<_> = clients
        .into_iter()
        .map(|mut client| {
            thread::spawn(move || {
                client
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let _ = client.write_all(&vec![b'x'; 160 * 1024]);
                let _ = client.shutdown(Shutdown::Write);
            })
        })
        .collect();
    for sender in senders {
        sender.join().unwrap();
    }
    release_tx.send(()).unwrap();
    helper.join().unwrap();
    let result = proxy.stop_and_join().unwrap();
    assert_byte_budget_poison(&result);
    let read: usize = result
        .slots
        .iter()
        .map(|slot| slot.client_to_upstream.bytes_read)
        .sum();
    assert!(read <= 256 * 1024);
    assert!(result.peak_client_to_upstream_pending > 0);
    assert!(result.peak_client_to_upstream_pending <= 16 * 1024);
    assert!(result.peak_upstream_to_client_pending <= 16 * 1024);
}

#[test]
fn upstream_to_client_budget_and_pending_buffers_are_shared() {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target = upstream.local_addr().unwrap();
    let (release_tx, release_rx) = mpsc::channel();
    let helper = thread::spawn(move || {
        let sockets = [upstream.accept().unwrap().0, upstream.accept().unwrap().0];
        release_rx.recv().unwrap();
        sockets
            .into_iter()
            .map(|socket| thread::spawn(move || write_fixture(socket)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|writer| writer.join().unwrap())
            .sum::<usize>()
    });
    let (proxy, clients) = open_two_tunnels(target);
    let readers: Vec<_> = clients
        .into_iter()
        .map(|mut client| thread::spawn(move || two_tunnel_support::read_to_end(&mut client).len()))
        .collect();
    release_tx.send(()).unwrap();
    let server_written = helper.join().unwrap();
    let client_received: usize = readers
        .into_iter()
        .map(|reader| reader.join().unwrap())
        .sum();
    let result = proxy.stop_and_join().unwrap();
    assert!(server_written > 256 * 1024);
    assert!(client_received <= 256 * 1024);
    assert_byte_budget_poison(&result);
    let read: usize = result
        .slots
        .iter()
        .map(|slot| slot.upstream_to_client.bytes_read)
        .sum();
    assert!(read <= 256 * 1024);
    assert!(
        result
            .slots
            .iter()
            .all(|slot| slot.upstream_to_client.bytes_read > 0)
    );
    assert!(result.peak_upstream_to_client_pending > 0);
    assert!(result.peak_upstream_to_client_pending <= 16 * 1024);
    assert!(result.peak_client_to_upstream_pending <= 16 * 1024);
}

fn open_two_tunnels(target: SocketAddr) -> (TwoTunnelProxy, [TcpStream; 2]) {
    let proxy =
        TwoTunnelProxy::start(target, NONCE, Instant::now() + Duration::from_secs(6)).unwrap();
    let mut probe = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy.port())).unwrap();
    probe.write_all(PREFLIGHT).unwrap();
    let _ = two_tunnel_support::read_to_end(&mut probe);
    let clients = [
        two_tunnel_support::connect_tunnel(proxy.port()),
        two_tunnel_support::connect_tunnel(proxy.port()),
    ];
    (proxy, clients)
}

fn write_fixture(mut socket: TcpStream) -> usize {
    let payload = vec![b'y'; 160 * 1024];
    let mut written = 0;
    while written < payload.len() {
        match socket.write(&payload[written..]) {
            Ok(0) | Err(_) => break,
            Ok(count) => written += count,
        }
    }
    let _ = socket.shutdown(Shutdown::Write);
    written
}

fn assert_byte_budget_poison(result: &hf_platform::qualification::live::TwoTunnelOutcome) {
    assert!(result.failed && result.owned_sockets_closed && result.worker_joined);
    assert_eq!(
        result.first_failure.as_ref().unwrap().reason,
        TwoTunnelFailureReason::ByteBudget
    );
    assert_eq!(result.connects, 2);
    assert!(
        result
            .slots
            .iter()
            .all(|slot| slot.client_closed && slot.upstream_closed)
    );
}
