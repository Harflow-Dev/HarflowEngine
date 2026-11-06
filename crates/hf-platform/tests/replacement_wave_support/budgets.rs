use super::*;

#[test]
fn client_read_budget_covers_retired_slots_and_bounds_pending_bytes() {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target = upstream.local_addr().unwrap();
    let helper = thread::spawn(move || {
        for _ in 0..4 {
            let (mut stream, _) = upstream.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).unwrap();
            stream.shutdown(Shutdown::Write).unwrap();
        }
    });
    let proxy = TwoTunnelProxy::start_replacement_wave(
        target,
        NONCE,
        Instant::now() + Duration::from_secs(8),
    )
    .unwrap();
    fixtures::preflight(&proxy);
    for _ in 0..4 {
        let mut stream = fixtures::tunnel(proxy.port());
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        fixtures::established_one(&mut stream);
        let _ = stream.write_all(&vec![b'x'; 80 * 1024]);
        let _ = stream.shutdown(Shutdown::Write);
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response);
    }
    helper.join().unwrap();
    let result = proxy.stop_and_join().unwrap();
    assert_budget(&result, true);
}

#[test]
fn upstream_read_budget_covers_retired_slots_and_bounds_pending_bytes() {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target = upstream.local_addr().unwrap();
    let helper = thread::spawn(move || {
        for _ in 0..4 {
            let (mut stream, _) = upstream.accept().unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let _ = stream.write_all(&vec![b'y'; 80 * 1024]);
            let _ = stream.shutdown(Shutdown::Write);
            let mut rest = Vec::new();
            let _ = stream.read_to_end(&mut rest);
        }
    });
    let proxy = TwoTunnelProxy::start_replacement_wave(
        target,
        NONCE,
        Instant::now() + Duration::from_secs(8),
    )
    .unwrap();
    fixtures::preflight(&proxy);
    for _ in 0..4 {
        let mut stream = fixtures::tunnel(proxy.port());
        fixtures::established_one(&mut stream);
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response);
    }
    helper.join().unwrap();
    let result = proxy.stop_and_join().unwrap();
    assert_budget(&result, false);
}

fn assert_budget(result: &hf_platform::qualification::live::TwoTunnelOutcome, outgoing: bool) {
    use hf_platform::qualification::live::TwoTunnelFailureReason;
    assert!(result.failed && result.owned_sockets_closed && result.worker_joined);
    assert_eq!(
        result.first_failure.as_ref().unwrap().reason,
        TwoTunnelFailureReason::ByteBudget
    );
    let total = result
        .slots
        .iter()
        .map(|slot| {
            if outgoing {
                slot.client_to_upstream.bytes_read
            } else {
                slot.upstream_to_client.bytes_read
            }
        })
        .sum::<usize>();
    assert_eq!(total, 256 * 1024);
    assert!(result.peak_client_to_upstream_pending <= 16 * 1024);
    assert!(result.peak_upstream_to_client_pending <= 16 * 1024);
}
