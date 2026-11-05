#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_interfaces::backend_measurement::{anthropic_text_response, decode_anthropic_request};
#[path = "http.rs"]
mod http;
use std::{
    io,
    net::TcpListener,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const EXCHANGE_LIMIT: Duration = Duration::from_secs(120);
const FAKE_KEY: &str = "harflow-fake-fixture-key";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ServerOutcome {
    NoRequest,
    PreflightOnly,
    Completed,
}

pub(super) struct FixtureServer {
    port: u16,
    ipv6_guard: TcpListener,
    stop: Option<Sender<()>>,
    worker: Option<JoinHandle<Result<ServerOutcome, String>>>,
}
impl FixtureServer {
    pub(super) fn start(nonce: String) -> io::Result<Self> {
        let (listener, ipv6_guard, port) = reserve_loopback_pair()?;
        let (stop, stopped) = mpsc::channel();
        let worker = thread::spawn(move || serve(listener, stopped, nonce));
        Ok(Self {
            port,
            ipv6_guard,
            stop: Some(stop),
            worker: Some(worker),
        })
    }
    pub(super) fn port(&self) -> u16 {
        self.port
    }
    pub(super) fn no_ipv6_connection(&self) -> bool {
        matches!(self.ipv6_guard.accept(), Err(ref error) if error.kind() == io::ErrorKind::WouldBlock)
    }
    pub(super) fn stop_and_join(&mut self) -> ServerOutcome {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        join_bounded(self.worker.take().expect("owned fixture worker"))
            .expect("fixture server thread completed within deadline")
            .expect("fixture server accepted only its bounded protocol")
    }
}

fn reserve_loopback_pair() -> io::Result<(TcpListener, TcpListener, u16)> {
    for _ in 0..16 {
        let ipv4 = TcpListener::bind(("127.0.0.1", 0))?;
        ipv4.set_nonblocking(true)?;
        let port = ipv4.local_addr()?.port();
        match TcpListener::bind(("::1", port)) {
            Ok(ipv6) => {
                ipv6.set_nonblocking(true)?;
                return Ok((ipv4, ipv6, port));
            }
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => drop(ipv4),
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        "could not reserve an exclusive IPv4/IPv6 loopback pair within 16 attempts",
    ))
}
impl Drop for FixtureServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take()
            && join_bounded(worker).is_err()
        {
            eprintln!("fixture server thread panicked");
        }
    }
}

pub(super) struct DeniedListener {
    listener: TcpListener,
    port: u16,
}
impl DeniedListener {
    pub(super) fn bind(allowed_port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        if port == allowed_port {
            return Err(io::Error::other("fixture endpoints collided"));
        }
        Ok(Self { listener, port })
    }
    pub(super) fn port(&self) -> u16 {
        self.port
    }
    pub(super) fn no_connection(&self) -> bool {
        matches!(self.listener.accept(), Err(ref error) if error.kind() == io::ErrorKind::WouldBlock)
    }
}

fn serve(
    listener: TcpListener,
    stop: Receiver<()>,
    nonce: String,
) -> Result<ServerOutcome, String> {
    let end = Instant::now() + EXCHANGE_LIMIT;
    let mut preflight = false;
    let mut completed = false;
    loop {
        if Instant::now() >= end {
            return Err("fixture listener deadline expired".into());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_millis(250)))
                    .map_err(show)?;
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .map_err(show)?;
                let request = http::read_request(&mut stream, end)?;
                let route = format!("/{nonce}/preflight");
                if !preflight
                    && !completed
                    && request.method == "GET"
                    && request.path == route
                    && request.body.is_empty()
                {
                    http::reply(&mut stream, b"preflight-ok")?;
                    preflight = true;
                } else if preflight
                    && !completed
                    && request.method == "POST"
                    && request.path == format!("/{nonce}/v1/messages")
                {
                    if request.headers.get("x-api-key").map(String::as_str) != Some(FAKE_KEY) {
                        return Err("fixture POST lacks exact fake authentication".into());
                    }
                    let parsed = decode_anthropic_request(&request.body, &nonce)
                        .map_err(|error| format!("fixture codec rejected request: {error:?}"))?;
                    let body = anthropic_text_response(&parsed.model, &nonce)
                        .map_err(|error| format!("fixture response refused: {error:?}"))?;
                    http::reply(&mut stream, &body)?;
                    completed = true;
                } else {
                    return Err(
                        "fixture listener rejected unexpected method, route, order or count".into(),
                    );
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if stopped(&stop)? {
                    return Ok(if completed {
                        ServerOutcome::Completed
                    } else if preflight {
                        ServerOutcome::PreflightOnly
                    } else {
                        ServerOutcome::NoRequest
                    });
                }
                thread::yield_now();
            }
            Err(error) => return Err(show(error)),
        }
    }
}

fn stopped(stop: &Receiver<()>) -> Result<bool, String> {
    match stop.try_recv() {
        Ok(()) | Err(TryRecvError::Disconnected) => Ok(true),
        Err(TryRecvError::Empty) => Ok(false),
    }
}

fn join_bounded<T: Send + 'static>(worker: JoinHandle<T>) -> Result<T, String> {
    let end = Instant::now() + EXCHANGE_LIMIT + Duration::from_secs(2);
    while !worker.is_finished() && Instant::now() < end {
        thread::yield_now();
    }
    if !worker.is_finished() {
        std::process::abort();
    }
    worker
        .join()
        .map_err(|_| "fixture server thread panicked".into())
}
fn show(error: impl std::fmt::Display) -> String {
    error.to_string()
}
