use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, ToSocketAddrs},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
#[path = "relay.rs"]
mod relay;
#[derive(Clone, Copy, Debug)]
pub enum ProxyMode {
    Live,
    UntrustedTls(SocketAddr),
    Reject,
}
#[derive(Default, Debug, Clone)]
pub struct ProxyOutcome {
    pub upstream_peer: Option<SocketAddr>,
    pub preflights: usize,
    pub connects: usize,
    pub rejected_connections: usize,
    pub ipv6_connections: usize,
    pub client_eof: bool,
    pub remote_eof: bool,
    pub client_to_upstream_flushed: bool,
    pub owned_sockets_closed: bool,
    pub failed: bool,
}
pub struct Proxy {
    port: u16,
    stop: Arc<AtomicBool>,
    ipv6: Arc<AtomicBool>,
    join: Option<JoinHandle<ProxyOutcome>>,
}
impl Proxy {
    /// DNS/connect are nonpreemptible OS calls; deadlines are checked before/after.
    pub fn start(mode: ProxyMode, nonce: &str, until: Instant) -> io::Result<Self> {
        if nonce.is_empty()
            || nonce.len() > 128
            || !nonce
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || Instant::now() >= until
        {
            return Err(io::Error::other("proxy input refused"));
        }
        let upstream = resolve(mode, until)?;
        let v4 = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = v4.local_addr()?.port();
        let v6 = TcpListener::bind((Ipv6Addr::LOCALHOST, port))?;
        v4.set_nonblocking(true)?;
        v6.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let ipv6 = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let seen6 = Arc::clone(&ipv6);
        let nonce = nonce.to_owned();
        let join = thread::spawn(move || serve(v4, v6, upstream, &nonce, until, &stopped, &seen6));
        Ok(Self {
            port,
            stop,
            ipv6,
            join: Some(join),
        })
    }
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn quiet_ipv6(&self) -> bool {
        !self.ipv6.load(Ordering::SeqCst)
    }
    pub fn stop_and_join(mut self) -> io::Result<ProxyOutcome> {
        self.stop.store(true, Ordering::SeqCst);
        self.join
            .take()
            .ok_or_else(|| io::Error::other("proxy join absent"))?
            .join()
            .map_err(|_| io::Error::other("proxy thread failed"))
    }
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
fn resolve(mode: ProxyMode, until: Instant) -> io::Result<Option<SocketAddr>> {
    let address = match mode {
        ProxyMode::Reject => return Ok(None),
        ProxyMode::UntrustedTls(address) if address.ip().is_loopback() && address.port() != 0 => {
            address
        }
        ProxyMode::UntrustedTls(_) => return Err(io::Error::other("negative upstream refused")),
        ProxyMode::Live => ("api.anthropic.com", 443)
            .to_socket_addrs()?
            .find(|a| public(a.ip()))
            .ok_or_else(|| io::Error::other("public upstream absent"))?,
    };
    if Instant::now() >= until {
        return Err(io::Error::other("upstream resolution deadline"));
    }
    Ok(Some(address))
}
fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let [a, b, c, _] = v.octets();
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_broadcast()
                && !v.is_documentation()
                && a != 0
                && a < 224
                && !(a == 100 && (64..128).contains(&b))
                && !(a == 192 && b == 0 && c == 0)
                && !(a == 198 && (b == 18 || b == 19))
                && !(a == 192 && b == 88 && c == 99)
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            s[0] & 0xe000 == 0x2000
                && s[0] != 0x2002
                && s[0] != 0x3fff
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && v.to_ipv4_mapped().is_none()
        }
    }
}
fn serve(
    v4: TcpListener,
    v6: TcpListener,
    upstream: Option<SocketAddr>,
    nonce: &str,
    until: Instant,
    stop: &AtomicBool,
    seen6: &AtomicBool,
) -> ProxyOutcome {
    let mut outcome = ProxyOutcome::default();
    'serve: while !stop.load(Ordering::SeqCst) && Instant::now() < until && !outcome.failed {
        for (listener, ipv6) in [(&v4, false), (&v6, true)] {
            match listener.accept() {
                Ok((mut client, _)) => {
                    if ipv6 {
                        seen6.store(true, Ordering::SeqCst);
                        outcome.ipv6_connections += 1;
                    }
                    if outcome.connects > 0 || ipv6 {
                        outcome.rejected_connections += 1;
                        outcome.failed = true;
                        break 'serve;
                    }
                    if relay::handle(
                        &mut client,
                        upstream,
                        nonce,
                        until,
                        stop,
                        &mut outcome,
                        (&v4, &v6, seen6),
                    )
                    .is_err()
                    {
                        outcome.failed = true;
                        break 'serve;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
                Err(_) => {
                    outcome.failed = true;
                    break 'serve;
                }
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    // Include already queued attempts before relinquishing the exclusive pair.
    for (listener, is6) in [(&v4, false), (&v6, true)] {
        for _ in 0..1024 {
            match listener.accept() {
                Ok((stream, _)) => {
                    drop(stream);
                    outcome.rejected_connections += 1;
                    outcome.failed = true;
                    if is6 {
                        seen6.store(true, Ordering::SeqCst);
                        outcome.ipv6_connections += 1;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    outcome.failed = true;
                    break;
                }
            }
        }
    }
    outcome.owned_sockets_closed = true;
    outcome
}
