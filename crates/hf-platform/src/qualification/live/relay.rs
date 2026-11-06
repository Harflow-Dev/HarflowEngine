use super::ProxyOutcome;
use std::{
    io::{self, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
fn check(until: Instant, stop: &AtomicBool) -> io::Result<()> {
    if Instant::now() >= until || stop.load(Ordering::SeqCst) {
        Err(io::Error::other("proxy stopped"))
    } else {
        Ok(())
    }
}
fn write(
    stream: &mut TcpStream,
    bytes: &[u8],
    until: Instant,
    stop: &AtomicBool,
) -> io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        check(until, stop)?;
        match stream.write(&bytes[offset..]) {
            Ok(0) => return Err(io::Error::other("proxy write closed")),
            Ok(n) => offset += n,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20))
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
fn header(client: &mut TcpStream, until: Instant, stop: &AtomicBool) -> io::Result<String> {
    let mut bytes = Vec::new();
    let mut byte = [0];
    while bytes.len() < 8192 {
        check(until, stop)?;
        match client.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                bytes.push(byte[0]);
                if bytes.ends_with(b"\r\n\r\n") {
                    return String::from_utf8(bytes)
                        .map_err(|_| io::Error::other("proxy header refused"));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20))
            }
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("proxy header budget"))
}
fn route(header: &str, nonce: &str) -> io::Result<bool> {
    let mut lines = header.split("\r\n");
    let first = lines.next().unwrap_or_default();
    let connect = first == "CONNECT api.anthropic.com:443 HTTP/1.1";
    if !connect && first != format!("GET /{nonce}/preflight HTTP/1.1") {
        return Err(io::Error::other("proxy route refused"));
    }
    let mut names = Vec::new();
    for line in lines.filter(|s| !s.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| io::Error::other("proxy header refused"))?;
        let name = name.to_ascii_lowercase();
        if names.contains(&name)
            || !matches!(
                name.as_str(),
                "host" | "connection" | "proxy-connection" | "user-agent"
            )
            || value.chars().any(char::is_control)
        {
            return Err(io::Error::other("proxy header refused"));
        }
        if name == "host" && connect && value.trim() != "api.anthropic.com:443" {
            return Err(io::Error::other("proxy host refused"));
        }
        names.push(name);
    }
    if connect && !names.iter().any(|name| name == "host") {
        return Err(io::Error::other("proxy host absent"));
    }
    Ok(connect)
}
pub(super) fn handle(
    client: &mut TcpStream,
    upstream: Option<SocketAddr>,
    nonce: &str,
    until: Instant,
    stop: &AtomicBool,
    outcome: &mut ProxyOutcome,
    listeners: (&TcpListener, &TcpListener, &AtomicBool),
) -> io::Result<()> {
    client.set_nonblocking(true)?;
    let connect = route(&header(client, until, stop)?, nonce)?;
    if !connect {
        if outcome.preflights != 0 {
            return Err(io::Error::other("duplicate proxy preflight"));
        }
        outcome.preflights += 1;
        write(
            client,
            b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\npreflight-ok",
            until,
            stop,
        )?;
        return Ok(());
    }
    if outcome.preflights != 1 {
        return Err(io::Error::other("proxy preflight absent"));
    }
    outcome.connects += 1;
    let address = upstream.ok_or_else(|| io::Error::other("proxy upstream refused"))?;
    check(until, stop)?;
    let mut remote = TcpStream::connect_timeout(
        &address,
        until
            .saturating_duration_since(Instant::now())
            .min(Duration::from_secs(5)),
    )?;
    check(until, stop)?;
    let peer = remote.peer_addr()?;
    check(until, stop)?;
    if peer != address {
        return Err(io::Error::other("proxy upstream peer mismatch"));
    }
    outcome.upstream_peer = Some(peer);
    remote.set_nonblocking(true)?;
    write(
        client,
        b"HTTP/1.1 200 Connection Established\r\n\r\n",
        until,
        stop,
    )?;
    let result = tunnel(client, &mut remote, until, stop, outcome, listeners);
    let _ = remote.shutdown(Shutdown::Both);
    let _ = client.shutdown(Shutdown::Both);
    result
}
struct Direction {
    buffer: Vec<u8>,
    offset: usize,
    total: usize,
    eof: bool,
    closed: bool,
}
impl Direction {
    fn new() -> Self {
        Self {
            buffer: Vec::new(),
            offset: 0,
            total: 0,
            eof: false,
            closed: false,
        }
    }
    fn step(&mut self, source: &mut TcpStream, target: &mut TcpStream) -> io::Result<()> {
        if self.offset < self.buffer.len() {
            match target.write(&self.buffer[self.offset..]) {
                Ok(0) => return Err(io::Error::other("tunnel write closed")),
                Ok(n) => self.offset += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
                Err(e) => return Err(e),
            }
        }
        if self.offset != self.buffer.len() {
            return Ok(());
        }
        self.buffer.clear();
        self.offset = 0;
        if self.eof {
            if !self.closed {
                target.shutdown(Shutdown::Write)?;
                self.closed = true;
            }
            return Ok(());
        }
        let mut bytes = [0; 16384];
        match source.read(&mut bytes) {
            Ok(0) => self.eof = true,
            Ok(n) => {
                self.total += n;
                if self.total > 262144 {
                    return Err(io::Error::other("tunnel byte budget"));
                }
                self.buffer.extend_from_slice(&bytes[..n]);
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
            Err(e) => return Err(e),
        }
        Ok(())
    }
}
fn tunnel(
    client: &mut TcpStream,
    remote: &mut TcpStream,
    until: Instant,
    stop: &AtomicBool,
    outcome: &mut ProxyOutcome,
    listeners: (&TcpListener, &TcpListener, &AtomicBool),
) -> io::Result<()> {
    let mut outgoing = Direction::new();
    let mut incoming = Direction::new();
    loop {
        for (listener, is6) in [(listeners.0, false), (listeners.1, true)] {
            if let Ok((stream, _)) = listener.accept() {
                drop(stream);
                outcome.rejected_connections += 1;
                outcome.failed = true;
                if is6 {
                    listeners.2.store(true, Ordering::SeqCst);
                    outcome.ipv6_connections += 1;
                }
            }
        }
        if outcome.failed {
            return Err(io::Error::other("additional proxy connection refused"));
        }
        outgoing.step(client, remote)?;
        incoming.step(remote, client)?;
        outcome.client_eof = outgoing.eof;
        outcome.remote_eof = incoming.eof;
        outcome.client_to_upstream_flushed = outgoing.eof && outgoing.buffer.is_empty();
        if outgoing.closed && incoming.closed {
            return Ok(());
        }
        if stop.load(Ordering::SeqCst) && outgoing.closed && incoming.buffer.is_empty() {
            return Ok(());
        }
        check(until, stop)?;
        thread::sleep(Duration::from_millis(20));
    }
}
