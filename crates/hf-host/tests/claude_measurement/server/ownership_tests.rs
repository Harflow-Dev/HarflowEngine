use super::*;
use std::{
    io::{Read, Write},
    net::{Shutdown, TcpStream},
    time::Duration,
};

#[test]
fn fixture_route_accepts_only_canonical_path_or_exact_sdk_beta_query() {
    let nonce = "routecontractnonce1234";
    for path in [
        format!("/{nonce}/v1/messages"),
        format!("/{nonce}/v1/messages?beta=true"),
    ] {
        assert!(super::is_fixture_request(
            false,
            true,
            false,
            &request("POST", &path),
            nonce
        ));
        assert!(!super::is_fixture_request(
            true,
            true,
            false,
            &request("POST", &path),
            nonce
        ));
        assert!(!super::is_fixture_request(
            false,
            false,
            false,
            &request("POST", &path),
            nonce
        ));
        assert!(!super::is_fixture_request(
            false,
            true,
            true,
            &request("POST", &path),
            nonce
        ));
    }

    for path in [
        format!("/{nonce}/v1/messages?beta=false"),
        format!("/{nonce}/v1/messages?other=true"),
        format!("/{nonce}/v1/messages?beta=true&beta=true"),
        format!("/{nonce}/v1/messages?beta=true&other=true"),
        format!("/{nonce}/v1/messages?count_tokens=true"),
        format!("/{nonce}/v1/messages%3Fbeta=true"),
        format!("/{nonce}/preflight?beta=true"),
    ] {
        assert!(
            !super::is_fixture_request(false, true, false, &request("POST", &path), nonce),
            "unexpectedly accepted route {path}"
        );
    }
    assert!(!super::is_preflight_request(
        false,
        false,
        &request("GET", &format!("/{nonce}/preflight?beta=true")),
        nonce
    ));
}

fn request(method: &str, path: &str) -> http::Request {
    http::Request {
        method: method.into(),
        path: path.into(),
        headers: Default::default(),
        body: Vec::new(),
    }
}

#[test]
#[ignore = "controlled loopback ownership probe; requires explicit primary-host socket authorization"]
fn wrong_route_closes_worker_and_parent_guard_retains_ipv4_port() {
    let mut server = FixtureServer::start("server-ownership-test-nonce".into()).unwrap();
    let port = server.port();
    let mut client =
        TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .write_all(b"GET /wrong-route HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    client.shutdown(Shutdown::Write).unwrap();

    let mut drained = Vec::new();
    let mut chunk = [0_u8; 256];
    loop {
        let count = client.read(&mut chunk).unwrap();
        if count == 0 {
            break;
        }
        assert!(
            drained.len() + count <= 1024,
            "unexpected server response exceeded drain bound"
        );
        drained.extend_from_slice(&chunk[..count]);
    }
    assert!(
        drained.is_empty(),
        "unexpected route must not receive a response"
    );
    assert!(
        server.stop_and_join_result().is_err(),
        "server must report the rejected route"
    );
    assert_eq!(
        TcpListener::bind(("127.0.0.1", port)).unwrap_err().kind(),
        io::ErrorKind::AddrInUse,
        "parent guard must retain the IPv4 port after worker settlement"
    );
}
