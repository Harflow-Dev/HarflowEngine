//! Capture-free, frozen-loopback-only two-tunnel negative control.
mod state;
mod worker;
pub use state::{
    TwoTunnelDirection, TwoTunnelFailure, TwoTunnelFailureReason, TwoTunnelOutcome,
    TwoTunnelSlotOutcome,
};
use std::{
    io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Instant,
};
pub struct TwoTunnelProxy {
    port: u16,
    stop: Arc<AtomicBool>,
    ipv6: Arc<AtomicBool>,
    settled: Arc<AtomicBool>,
    join: Option<JoinHandle<TwoTunnelOutcome>>,
}
impl TwoTunnelProxy {
    pub fn start(target: SocketAddr, nonce: &str, until: Instant) -> io::Result<Self> {
        Self::start_mode(target, nonce, until, worker::Mode::Original)
    }
    pub fn start_replacement_wave(
        target: SocketAddr,
        nonce: &str,
        until: Instant,
    ) -> io::Result<Self> {
        Self::start_mode(target, nonce, until, worker::Mode::ReplacementWave)
    }
    fn start_mode(
        target: SocketAddr,
        nonce: &str,
        until: Instant,
        mode: worker::Mode,
    ) -> io::Result<Self> {
        if target.ip() != Ipv4Addr::LOCALHOST
            || target.port() == 0
            || nonce.is_empty()
            || nonce.len() > 128
            || !nonce
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || Instant::now() >= until
        {
            return Err(io::Error::other("two tunnel input refused"));
        }
        let v4 = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = v4.local_addr()?.port();
        let v6 = TcpListener::bind((Ipv6Addr::LOCALHOST, port))?;
        v4.set_nonblocking(true)?;
        v6.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let ipv6 = Arc::new(AtomicBool::new(false));
        let settled = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&settled);
        let stopped = Arc::clone(&stop);
        let seen6 = Arc::clone(&ipv6);
        let nonce = nonce.to_owned();
        let join = thread::spawn(move || {
            worker::serve(
                v4,
                v6,
                target,
                nonce,
                until,
                (&stopped, &observed, &seen6),
                mode,
            )
        });
        Ok(Self {
            port,
            stop,
            ipv6,
            settled,
            join: Some(join),
        })
    }
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn quiet_ipv6(&self) -> bool {
        !self.ipv6.load(Ordering::SeqCst)
    }
    /// Advisory observation only; the joined outcome remains authoritative.
    pub fn observation_ready(&self) -> bool {
        self.settled.load(Ordering::Acquire)
            || self.join.as_ref().is_some_and(JoinHandle::is_finished)
    }
    pub fn stop_and_join(mut self) -> io::Result<TwoTunnelOutcome> {
        self.stop.store(true, Ordering::SeqCst);
        let mut result = self
            .join
            .take()
            .ok_or_else(|| io::Error::other("join absent"))?
            .join()
            .map_err(|_| io::Error::other("worker failed"))?;
        result.worker_joined = true;
        Ok(result)
    }
}
impl Drop for TwoTunnelProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
